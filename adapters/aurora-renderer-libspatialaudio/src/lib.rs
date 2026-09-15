//! Runtime libspatialaudio adapter behind Aurora's additive object-PCM renderer contract.
//!
//! The adapter dynamically loads an Aurora-owned C++ shim. Ordinary Aurora
//! builds therefore do not link libspatialaudio or expose its C++ ABI. The shim
//! is compiled against the exact pinned upstream revision in dedicated CI.

use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{ObjectPcmBlock, ObjectPcmRenderer, PcmRendererError};

const SHIM_ABI_VERSION: u32 = 1;
const MEDIA_RATE_HZ: u32 = 48_000;
const BLOCK_FRAMES: usize = 256;
const OUTPUT_CHANNELS: usize = 12;
const DIRECT_PATH_LATENCY_FRAMES: usize = 255;
const ORIENTATION_EPSILON: f32 = 1.0e-5;

/// Setup-time configuration for the dynamically loaded native shim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibspatialaudioRuntimeConfig {
    /// Path to the Aurora-owned shim shared library.
    pub shim_path: PathBuf,
}

impl LibspatialaudioRuntimeConfig {
    /// Creates a runtime configuration for one exact shim library.
    pub fn new(shim_path: impl Into<PathBuf>) -> Self {
        Self {
            shim_path: shim_path.into(),
        }
    }
}

/// Fail-closed adapter load errors before renderer configuration.
#[derive(Debug)]
pub enum LibspatialaudioLoadError {
    /// Shared-library path cannot be represented for the platform loader.
    InvalidLibraryPath,
    /// Platform dynamic loader returned an error.
    DynamicLibrary(String),
    /// Required Aurora shim symbol is missing.
    MissingSymbol(String),
    /// Loaded shim ABI does not match this adapter.
    AbiMismatch { expected: u32, actual: u32 },
    /// Shim could not construct a native renderer handle.
    CreateFailed,
}

impl fmt::Display for LibspatialaudioLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPath => write!(f, "invalid libspatialaudio shim library path"),
            Self::DynamicLibrary(message) => {
                write!(f, "libspatialaudio shim load error: {message}")
            }
            Self::MissingSymbol(name) => write!(f, "libspatialaudio shim missing symbol {name}"),
            Self::AbiMismatch { expected, actual } => write!(
                f,
                "libspatialaudio shim ABI mismatch: expected {expected}, got {actual}"
            ),
            Self::CreateFailed => write!(f, "libspatialaudio shim failed to create a handle"),
        }
    }
}

impl std::error::Error for LibspatialaudioLoadError {}

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
type ConfigureFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> i32;
type RenderFn = unsafe extern "C" fn(
    *mut c_void,
    u32,
    *const f32,
    *const f32,
    *const *const f32,
    *mut *mut f32,
) -> i32;
type ResetFn = unsafe extern "C" fn(*mut c_void) -> i32;
type OutputChannelsFn = unsafe extern "C" fn(*mut c_void) -> u32;
type LatencyFramesFn = unsafe extern "C" fn() -> u32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct ShimApi {
    configure: ConfigureFn,
    render: RenderFn,
    reset: ResetFn,
    output_channels: OutputChannelsFn,
    latency_frames: LatencyFramesFn,
    destroy: DestroyFn,
}

/// Exact pinned libspatialaudio object-PCM renderer adapter.
pub struct LibspatialaudioRenderer {
    library: SharedLibrary,
    api: ShimApi,
    handle: *mut c_void,
    configured: bool,
    max_objects: usize,
    positions_xyz: Vec<f32>,
    gains: Vec<f32>,
    input_ptrs: Vec<*const f32>,
    output_ptrs: Vec<*mut f32>,
}

// The raw pointers are preallocated scratch owned by this instance and are only
// populated/used synchronously on the render thread during `render_pcm`.
unsafe impl Send for LibspatialaudioRenderer {}

impl LibspatialaudioRenderer {
    /// Loads the Aurora-owned shim and creates one native renderer instance.
    pub fn load(
        config: LibspatialaudioRuntimeConfig,
    ) -> Result<Self, LibspatialaudioLoadError> {
        let library = SharedLibrary::open(&config.shim_path)?;
        let abi_version: AbiVersionFn =
            unsafe { library.symbol(b"aurora_spaudio_abi_version\0")? };
        let actual = unsafe { abi_version() };
        if actual != SHIM_ABI_VERSION {
            return Err(LibspatialaudioLoadError::AbiMismatch {
                expected: SHIM_ABI_VERSION,
                actual,
            });
        }
        let create: CreateFn = unsafe { library.symbol(b"aurora_spaudio_create\0")? };
        let api = ShimApi {
            configure: unsafe { library.symbol(b"aurora_spaudio_configure\0")? },
            render: unsafe { library.symbol(b"aurora_spaudio_render\0")? },
            reset: unsafe { library.symbol(b"aurora_spaudio_reset\0")? },
            output_channels: unsafe { library.symbol(b"aurora_spaudio_output_channels\0")? },
            latency_frames: unsafe { library.symbol(b"aurora_spaudio_latency_frames\0")? },
            destroy: unsafe { library.symbol(b"aurora_spaudio_destroy\0")? },
        };
        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(LibspatialaudioLoadError::CreateFailed);
        }
        Ok(Self {
            library,
            api,
            handle,
            configured: false,
            max_objects: 0,
            positions_xyz: Vec::new(),
            gains: Vec::new(),
            input_ptrs: Vec::new(),
            output_ptrs: Vec::new(),
        })
    }

    fn native_ok(code: i32) -> Result<(), PcmRendererError> {
        if code == 0 {
            Ok(())
        } else {
            Err(PcmRendererError::BackendFault)
        }
    }

    fn validate_layout(layout: &[Speaker]) -> Result<(), PcmRendererError> {
        let enabled_count = layout.iter().filter(|speaker| speaker.enabled).count();
        if enabled_count != OUTPUT_CHANNELS {
            return Err(PcmRendererError::InvalidConfiguration(format!(
                "libspatialaudio v1 requires exactly {OUTPUT_CHANNELS} enabled speakers"
            )));
        }
        for (index, speaker) in layout.iter().filter(|speaker| speaker.enabled).enumerate() {
            if !canonical_role_matches(index, &speaker.channel_role) {
                return Err(PcmRendererError::InvalidConfiguration(format!(
                    "libspatialaudio v1 requires canonical 7.1.4 role order; index {index} is {}",
                    speaker.channel_role
                )));
            }
        }
        Ok(())
    }

    fn fill_listener_relative_metadata(
        &mut self,
        listener: &Listener,
        objects: &[ObjectPcmBlock<'_>],
    ) -> Result<(), PcmRendererError> {
        if !vector_is_finite(listener.position)
            || !vector_is_finite(listener.orientation)
            || !listener.ear_height.is_finite()
        {
            return Err(PcmRendererError::NonFiniteMetadata);
        }
        if listener.orientation.z.abs() > ORIENTATION_EPSILON {
            return Err(PcmRendererError::UnsupportedListenerOrientation);
        }
        let forward_length = listener
            .orientation
            .x
            .hypot(listener.orientation.y);
        if forward_length <= ORIENTATION_EPSILON {
            return Err(PcmRendererError::UnsupportedListenerOrientation);
        }
        let forward_x = listener.orientation.x / forward_length;
        let forward_y = listener.orientation.y / forward_length;
        // Aurora and libspatialaudio both use +X right, +Y front, +Z up. Rotate
        // room/world coordinates so the listener's horizontal forward vector
        // becomes libspatialaudio-local +Y without moving speaker output order.
        let right_x = forward_y;
        let right_y = -forward_x;

        for (index, object) in objects.iter().enumerate() {
            if !vector_is_finite(object.object.position)
                || !object.object.gain.is_finite()
                || object.samples.iter().any(|sample| !sample.is_finite())
            {
                return Err(PcmRendererError::NonFiniteMetadata);
            }
            let dx = object.object.position.x - listener.position.x;
            let dy = object.object.position.y - listener.position.y;
            let dz = object.object.position.z - listener.position.z;
            let base = index * 3;
            self.positions_xyz[base] = dx.mul_add(right_x, dy * right_y);
            self.positions_xyz[base + 1] = dx.mul_add(forward_x, dy * forward_y);
            self.positions_xyz[base + 2] = dz;
            self.gains[index] = object.object.gain;
            self.input_ptrs[index] = object.samples.as_ptr();
        }
        Ok(())
    }
}

impl ObjectPcmRenderer for LibspatialaudioRenderer {
    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), PcmRendererError> {
        if sample_rate != MEDIA_RATE_HZ {
            return Err(PcmRendererError::InvalidConfiguration(format!(
                "libspatialaudio adapter v1 requires {MEDIA_RATE_HZ} Hz"
            )));
        }
        if block_size != BLOCK_FRAMES {
            return Err(PcmRendererError::InvalidConfiguration(format!(
                "libspatialaudio adapter v1 requires {BLOCK_FRAMES}-frame blocks"
            )));
        }
        if max_objects == 0 || max_objects > u32::MAX as usize {
            return Err(PcmRendererError::InvalidConfiguration(
                "max_objects must be in 1..=u32::MAX".to_owned(),
            ));
        }
        Self::validate_layout(&layout)?;
        Self::native_ok(unsafe {
            (self.api.configure)(
                self.handle,
                sample_rate,
                block_size as u32,
                max_objects as u32,
            )
        })?;
        if unsafe { (self.api.output_channels)(self.handle) } as usize != OUTPUT_CHANNELS {
            return Err(PcmRendererError::BackendFault);
        }
        if unsafe { (self.api.latency_frames)() } as usize != DIRECT_PATH_LATENCY_FRAMES {
            return Err(PcmRendererError::BackendFault);
        }

        self.positions_xyz = vec![0.0; max_objects * 3];
        self.gains = vec![0.0; max_objects];
        self.input_ptrs = vec![std::ptr::null(); max_objects];
        self.output_ptrs = vec![std::ptr::null_mut(); OUTPUT_CHANNELS];
        self.max_objects = max_objects;
        self.configured = true;
        Ok(())
    }

    fn render_pcm(
        &mut self,
        listener: &Listener,
        objects: &[ObjectPcmBlock<'_>],
        output: &mut [&mut [f32]],
    ) -> Result<(), PcmRendererError> {
        if !self.configured {
            return Err(PcmRendererError::NotConfigured);
        }
        if objects.len() > self.max_objects {
            return Err(PcmRendererError::TooManyObjects {
                maximum: self.max_objects,
                actual: objects.len(),
            });
        }
        for (index, object) in objects.iter().enumerate() {
            if object.samples.len() != BLOCK_FRAMES {
                return Err(PcmRendererError::InputBlockSize {
                    object_index: index,
                    required: BLOCK_FRAMES,
                    actual: object.samples.len(),
                });
            }
        }
        if output.len() != OUTPUT_CHANNELS {
            return Err(PcmRendererError::OutputChannelCount {
                required: OUTPUT_CHANNELS,
                actual: output.len(),
            });
        }
        for (channel, samples) in output.iter().enumerate() {
            if samples.len() != BLOCK_FRAMES {
                return Err(PcmRendererError::OutputBlockSize {
                    channel,
                    required: BLOCK_FRAMES,
                    actual: samples.len(),
                });
            }
        }

        self.fill_listener_relative_metadata(listener, objects)?;
        for (slot, channel) in self.output_ptrs.iter_mut().zip(output.iter_mut()) {
            channel.fill(0.0);
            *slot = channel.as_mut_ptr();
        }
        Self::native_ok(unsafe {
            (self.api.render)(
                self.handle,
                objects.len() as u32,
                self.positions_xyz.as_ptr(),
                self.gains.as_ptr(),
                self.input_ptrs.as_ptr(),
                self.output_ptrs.as_mut_ptr(),
            )
        })
    }

    fn reset(&mut self) {
        if self.configured {
            let _ = unsafe { (self.api.reset)(self.handle) };
        }
    }

    fn latency_frames(&self) -> usize {
        DIRECT_PATH_LATENCY_FRAMES
    }

    fn output_channel_count(&self) -> usize {
        if self.configured {
            OUTPUT_CHANNELS
        } else {
            0
        }
    }
}

impl Drop for LibspatialaudioRenderer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        // Keep the shared object loaded until after native destruction.
        let _ = &self.library;
    }
}

fn canonical_role_matches(index: usize, role: &ChannelRole) -> bool {
    matches!(
        (index, role),
        (0, ChannelRole::FrontLeft)
            | (1, ChannelRole::FrontRight)
            | (2, ChannelRole::FrontCenter)
            | (3, ChannelRole::LowFrequencyEffects)
            | (4, ChannelRole::SurroundLeft)
            | (5, ChannelRole::SurroundRight)
            | (6, ChannelRole::SurroundBackLeft)
            | (7, ChannelRole::SurroundBackRight)
            | (8, ChannelRole::TopFrontLeft)
            | (9, ChannelRole::TopFrontRight)
            | (10, ChannelRole::TopRearLeft)
            | (11, ChannelRole::TopRearRight)
    )
}

fn vector_is_finite(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

struct SharedLibrary {
    handle: *mut c_void,
}

unsafe impl Send for SharedLibrary {}

impl SharedLibrary {
    fn open(path: &Path) -> Result<Self, LibspatialaudioLoadError> {
        platform::open(path).map(|handle| Self { handle })
    }

    unsafe fn symbol<T: Copy>(
        &self,
        name: &'static [u8],
    ) -> Result<T, LibspatialaudioLoadError> {
        let c_name = CStr::from_bytes_with_nul(name)
            .map_err(|_| LibspatialaudioLoadError::MissingSymbol("invalid-symbol-name".into()))?;
        let ptr = platform::symbol(self.handle, c_name)?;
        if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err(LibspatialaudioLoadError::DynamicLibrary(
                "function pointer size does not match platform pointer size".into(),
            ));
        }
        Ok(std::mem::transmute_copy(&ptr))
    }
}

impl Drop for SharedLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { platform::close(self.handle) };
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;

    const RTLD_NOW: c_int = 2;

    #[link(name = "dl")]
    extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    pub fn open(path: &Path) -> Result<*mut c_void, LibspatialaudioLoadError> {
        let text = path
            .to_str()
            .ok_or(LibspatialaudioLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| LibspatialaudioLoadError::InvalidLibraryPath)?;
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(LibspatialaudioLoadError::DynamicLibrary(last_error()));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, LibspatialaudioLoadError> {
        let _ = dlerror();
        let ptr = dlsym(handle, name.as_ptr());
        let err = dlerror();
        if !err.is_null() {
            return Err(LibspatialaudioLoadError::MissingSymbol(
                name.to_string_lossy().into_owned(),
            ));
        }
        Ok(ptr)
    }

    pub unsafe fn close(handle: *mut c_void) {
        let _ = dlclose(handle);
    }

    fn last_error() -> String {
        let err = unsafe { dlerror() };
        if err.is_null() {
            return "unknown dynamic-loader error".into();
        }
        unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    type HModule = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const c_char) -> HModule;
        fn GetProcAddress(module: HModule, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: HModule) -> i32;
    }

    pub fn open(path: &Path) -> Result<*mut c_void, LibspatialaudioLoadError> {
        let text = path
            .to_str()
            .ok_or(LibspatialaudioLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| LibspatialaudioLoadError::InvalidLibraryPath)?;
        let handle = unsafe { LoadLibraryA(path.as_ptr()) };
        if handle.is_null() {
            return Err(LibspatialaudioLoadError::DynamicLibrary(
                "LoadLibraryA failed".into(),
            ));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, LibspatialaudioLoadError> {
        let ptr = GetProcAddress(handle, name.as_ptr());
        if ptr.is_null() {
            return Err(LibspatialaudioLoadError::MissingSymbol(
                name.to_string_lossy().into_owned(),
            ));
        }
        Ok(ptr)
    }

    pub unsafe fn close(handle: *mut c_void) {
        let _ = FreeLibrary(handle);
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;

    pub fn open(_path: &Path) -> Result<*mut c_void, LibspatialaudioLoadError> {
        Err(LibspatialaudioLoadError::DynamicLibrary(
            "unsupported dynamic-library platform".into(),
        ))
    }

    pub unsafe fn symbol(
        _handle: *mut c_void,
        _name: &CStr,
    ) -> Result<*mut c_void, LibspatialaudioLoadError> {
        Err(LibspatialaudioLoadError::DynamicLibrary(
            "unsupported dynamic-library platform".into(),
        ))
    }

    pub unsafe fn close(_handle: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_7_1_4_role_order_is_exact() {
        let roles = [
            ChannelRole::FrontLeft,
            ChannelRole::FrontRight,
            ChannelRole::FrontCenter,
            ChannelRole::LowFrequencyEffects,
            ChannelRole::SurroundLeft,
            ChannelRole::SurroundRight,
            ChannelRole::SurroundBackLeft,
            ChannelRole::SurroundBackRight,
            ChannelRole::TopFrontLeft,
            ChannelRole::TopFrontRight,
            ChannelRole::TopRearLeft,
            ChannelRole::TopRearRight,
        ];
        assert!(roles
            .iter()
            .enumerate()
            .all(|(index, role)| canonical_role_matches(index, role)));
        assert!(!canonical_role_matches(0, &ChannelRole::FrontRight));
    }

    #[test]
    fn listener_yaw_transform_maps_forward_to_positive_y() {
        let listener = Listener {
            position: Vector3::new(10.0, 20.0, 2.0),
            orientation: Vector3::new(1.0, 0.0, 0.0),
            ear_height: 1.2,
        };
        let object = Vector3::new(11.0, 20.0, 2.5);
        let dx = object.x - listener.position.x;
        let dy = object.y - listener.position.y;
        let len = listener.orientation.x.hypot(listener.orientation.y);
        let fx = listener.orientation.x / len;
        let fy = listener.orientation.y / len;
        let rx = fy;
        let ry = -fx;
        let local_x = dx.mul_add(rx, dy * ry);
        let local_y = dx.mul_add(fx, dy * fy);
        assert!(local_x.abs() < 1.0e-6);
        assert!((local_y - 1.0).abs() < 1.0e-6);
    }
}
