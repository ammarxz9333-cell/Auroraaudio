//! Runtime loader for Omniphony's stable `liborender.so` C ABI.
//!
//! Aurora R2 deliberately consumes the published ABI instead of patching or
//! linking private Omniphony Rust internals. This keeps the appliance host small,
//! pins the ABI major, and lets the decoder/render engine stay an independently
//! built shared library.

#![cfg(target_os = "linux")]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

const ORENDER_ABI_MAJOR: u32 = 0;
const RTLD_NOW: c_int = 2;

// Canonical Aurora 7.1.4 order in OrenderChannelLabel discriminants:
// L R C LFE Lb Rb Ls Rs Tfl Tfr Tbl Tbr.
const AURORA_LAYOUT: [u8; 12] = [0, 1, 2, 3, 14, 15, 4, 5, 6, 7, 10, 11];

#[repr(C)]
struct OrenderConfig {
    sample_rate: u32,
    config_yaml_path: *const c_char,
    speaker_layout_path: *const c_char,
    bridge_path: *const c_char,
    codec: *const c_char,
    osc_enabled: c_int,
    osc_port_in: u16,
    osc_port_out: u16,
    osc_bind: *const c_char,
    osc_host: *const c_char,
}

type VersionFn = unsafe extern "C" fn() -> u32;
type BuildIdFn = unsafe extern "C" fn() -> *const c_char;
type CreateFn = unsafe extern "C" fn(*const OrenderConfig) -> *mut c_void;
type DestroyFn = unsafe extern "C" fn(*mut c_void);
type ChannelCountFn = unsafe extern "C" fn(*const c_void) -> u32;
type ChannelLayoutFn = unsafe extern "C" fn(*const c_void, *mut u8, u32) -> u32;
type SetMappingFn = unsafe extern "C" fn(*mut c_void, c_int);
type SetChannelModeFn = unsafe extern "C" fn(*mut c_void, c_int);
type ProcessFn = unsafe extern "C" fn(
    *mut c_void,
    *const u8,
    usize,
    i64,
    *mut f32,
    usize,
    *mut usize,
    *mut u32,
    *mut i64,
) -> c_int;
type OutputLatencyFn = unsafe extern "C" fn(*const c_void) -> u64;
type HasObjectsFn = unsafe extern "C" fn(*const c_void) -> c_int;
type ObjectCountFn = unsafe extern "C" fn(*const c_void) -> c_int;

#[derive(Debug, Clone)]
pub struct OrenderHostConfig {
    pub library_path: String,
    pub bridge_path: String,
    pub speaker_layout_path: String,
    pub config_yaml_path: Option<String>,
}

pub struct OrenderHost {
    library: Library,
    renderer: *mut c_void,
    process: ProcessFn,
    output_latency: OutputLatencyFn,
    has_objects: HasObjectsFn,
    object_count: ObjectCountFn,
    render_buffer: Vec<f32>,
    build_id: String,
}

impl OrenderHost {
    pub fn open(config: &OrenderHostConfig) -> Result<Self, String> {
        let library = Library::open(&config.library_path)?;
        let version_major: VersionFn = library.required(b"orender_version_major\0")?;
        let version_minor: VersionFn = library.required(b"orender_version_minor\0")?;
        let build_id_fn: Option<BuildIdFn> = library.optional(b"orender_build_id\0")?;
        let create: CreateFn = library.required(b"orender_create\0")?;
        let destroy: DestroyFn = library.required(b"orender_destroy\0")?;
        let channel_count: ChannelCountFn = library.required(b"orender_channel_count\0")?;
        let channel_layout: ChannelLayoutFn = library.required(b"orender_channel_layout\0")?;
        let set_mapping: SetMappingFn = library.required(b"orender_set_channel_mapping\0")?;
        let set_channel_mode: SetChannelModeFn = library.required(b"orender_set_channel_mode\0")?;
        let process: ProcessFn = library.required(b"orender_process\0")?;
        let output_latency: OutputLatencyFn =
            library.required(b"orender_output_latency_samples\0")?;
        let has_objects: HasObjectsFn = library.required(b"orender_has_objects\0")?;
        let object_count: ObjectCountFn = library.required(b"orender_object_count\0")?;

        let major =
            // SAFETY: function pointer came from a symbol with this ABI.
            unsafe { version_major() };
        let minor =
            // SAFETY: function pointer came from a symbol with this ABI.
            unsafe { version_minor() };
        if major != ORENDER_ABI_MAJOR {
            return Err(format!(
                "unsupported liborender ABI {major}.{minor}; Aurora requires major {ORENDER_ABI_MAJOR}"
            ));
        }

        let bridge = cstring_path(&config.bridge_path, "bridge path")?;
        let layout = cstring_path(&config.speaker_layout_path, "speaker layout path")?;
        let yaml = config
            .config_yaml_path
            .as_deref()
            .map(|path| cstring_path(path, "config YAML path"))
            .transpose()?;
        let codec = CString::new("eac3").expect("static codec");
        let cfg = OrenderConfig {
            sample_rate: 48_000,
            config_yaml_path: yaml.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            speaker_layout_path: layout.as_ptr(),
            bridge_path: bridge.as_ptr(),
            codec: codec.as_ptr(),
            osc_enabled: 0,
            osc_port_in: 0,
            osc_port_out: 0,
            osc_bind: ptr::null(),
            osc_host: ptr::null(),
        };
        let renderer =
            // SAFETY: config pointers remain valid for the duration of create().
            unsafe { create(&cfg) };
        if renderer.is_null() {
            return Err("orender_create returned NULL".to_owned());
        }

        // Force spatial rendering and index-based output so the checked YAML
        // order is the physical TDM order. These are stable ABI calls.
        // SAFETY: renderer is a live handle returned by orender_create.
        unsafe {
            set_channel_mode(renderer, 1);
            set_mapping(renderer, 0);
        }

        let count =
            // SAFETY: renderer remains live.
            unsafe { channel_count(renderer) };
        if count != AURORA_LAYOUT.len() as u32 {
            // SAFETY: destroy matches the create function from this library.
            unsafe { destroy(renderer) };
            return Err(format!(
                "liborender produced {count} channels; Aurora R2 requires exactly {}",
                AURORA_LAYOUT.len()
            ));
        }
        let mut labels = [255_u8; 12];
        let layout_count =
            // SAFETY: labels has capacity for the already-verified channel count.
            unsafe { channel_layout(renderer, labels.as_mut_ptr(), labels.len() as u32) };
        if layout_count != labels.len() as u32 || labels != AURORA_LAYOUT {
            // SAFETY: destroy matches the create function from this library.
            unsafe { destroy(renderer) };
            return Err(format!(
                "liborender channel layout mismatch: got {labels:?}, expected {AURORA_LAYOUT:?}"
            ));
        }

        let build_id = build_id_fn
            .and_then(|function| {
                let raw =
                    // SAFETY: optional function pointer was successfully resolved.
                    unsafe { function() };
                (!raw.is_null()).then(|| {
                    // SAFETY: liborender documents a NUL-terminated build-id string.
                    unsafe { CStr::from_ptr(raw) }
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .unwrap_or_else(|| format!("ABI {major}.{minor}"));

        Ok(Self {
            library,
            renderer,
            process,
            output_latency,
            has_objects,
            object_count,
            // 12 * 1536 is the normal E-AC-3 upper bound; keep generous spare
            // capacity so the steady-state path never reallocates.
            render_buffer: vec![0.0; 65_536],
            build_id,
        })
    }

    pub fn build_id(&self) -> &str {
        &self.build_id
    }

    pub fn output_latency_samples(&self) -> u64 {
        // SAFETY: renderer and symbol remain live while self owns the library.
        unsafe { (self.output_latency)(self.renderer) }
    }

    pub fn has_objects(&self) -> bool {
        // SAFETY: live renderer handle.
        unsafe { (self.has_objects)(self.renderer) > 0 }
    }

    pub fn object_count(&self) -> u32 {
        let count =
            // SAFETY: live renderer handle.
            unsafe { (self.object_count)(self.renderer) };
        count.max(0) as u32
    }

    /// Decode/render one raw E-AC-3 packet and call `emit` with interleaved
    /// 12-channel f32 blocks. The sample slice is valid only during the callback.
    pub fn process_raw<F>(&mut self, packet: &[u8], mut emit: F) -> Result<(), String>
    where
        F: FnMut(&[f32], usize),
    {
        let mut attempts = 0;
        loop {
            let mut out_frames = 0usize;
            let mut out_channels = 0u32;
            let mut out_pts = 0i64;
            let status =
                // SAFETY: all pointers refer to live buffers/handle for this call.
                unsafe {
                    (self.process)(
                        self.renderer,
                        packet.as_ptr(),
                        packet.len(),
                        0,
                        self.render_buffer.as_mut_ptr(),
                        self.render_buffer.len(),
                        &mut out_frames,
                        &mut out_channels,
                        &mut out_pts,
                    )
                };
            if status < 0 {
                return Err(format!("orender_process failed with status {status}"));
            }
            if status > 0 {
                attempts += 1;
                if attempts > 4 || self.render_buffer.len() >= 1_048_576 {
                    return Err("liborender output exceeded Aurora safety buffer limit".to_owned());
                }
                self.render_buffer.resize(self.render_buffer.len() * 2, 0.0);
                continue;
            }
            if out_frames == 0 {
                return Ok(());
            }
            if out_channels != AURORA_LAYOUT.len() as u32 {
                return Err(format!(
                    "liborender changed output channel count mid-stream: {out_channels}"
                ));
            }
            let sample_count = out_frames
                .checked_mul(out_channels as usize)
                .ok_or_else(|| "liborender output size overflow".to_owned())?;
            if sample_count > self.render_buffer.len() {
                return Err("liborender reported samples beyond output capacity".to_owned());
            }
            emit(&self.render_buffer[..sample_count], out_frames);
            return Ok(());
        }
    }
}

impl Drop for OrenderHost {
    fn drop(&mut self) {
        if !self.renderer.is_null() {
            if let Ok(destroy) = self.library.required::<DestroyFn>(b"orender_destroy\0") {
                // SAFETY: renderer was created by this loaded library.
                unsafe { destroy(self.renderer) };
            }
            self.renderer = ptr::null_mut();
        }
    }
}

fn cstring_path(path: &str, description: &str) -> Result<CString, String> {
    if !Path::new(path).exists() {
        return Err(format!("{description} does not exist: {path}"));
    }
    CString::new(path).map_err(|_| format!("{description} contains NUL"))
}

struct Library {
    handle: *mut c_void,
}

impl Library {
    fn open(path: &str) -> Result<Self, String> {
        let path = cstring_path(path, "liborender path")?;
        let handle =
            // SAFETY: path is a valid C string and RTLD_NOW is a valid flag.
            unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(format!("dlopen(liborender) failed: {}", dl_error()));
        }
        Ok(Self { handle })
    }

    fn required<T: Copy>(&self, name: &'static [u8]) -> Result<T, String> {
        self.optional(name)?
            .ok_or_else(|| format!("required liborender symbol missing: {}", symbol_name(name)))
    }

    fn optional<T: Copy>(&self, name: &'static [u8]) -> Result<Option<T>, String> {
        let name_ptr = name.as_ptr().cast::<c_char>();
        // Clear any previous loader error before dlsym.
        // SAFETY: dlerror has no preconditions.
        unsafe { dlerror() };
        let raw =
            // SAFETY: handle is live and name is NUL-terminated static bytes.
            unsafe { dlsym(self.handle, name_ptr) };
        let error =
            // SAFETY: dlerror returns thread-local static error text or null.
            unsafe { dlerror() };
        if !error.is_null() {
            return Ok(None);
        }
        if raw.is_null() {
            return Ok(None);
        }
        if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err("internal symbol pointer size mismatch".to_owned());
        }
        // SAFETY: caller chooses the function-pointer type matching the public ABI.
        let function = unsafe { std::mem::transmute_copy::<*mut c_void, T>(&raw) };
        Ok(Some(function))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: this object owns the dlopen handle.
            unsafe { dlclose(self.handle) };
            self.handle = ptr::null_mut();
        }
    }
}

fn symbol_name(name: &[u8]) -> String {
    let end = name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name.len());
    String::from_utf8_lossy(&name[..end]).into_owned()
}

fn dl_error() -> String {
    let raw =
        // SAFETY: dlerror has no preconditions.
        unsafe { dlerror() };
    if raw.is_null() {
        "unknown dynamic loader error".to_owned()
    } else {
        // SAFETY: non-null result is NUL-terminated static storage.
        unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned()
    }
}

#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}
