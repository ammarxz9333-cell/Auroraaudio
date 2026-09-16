//! Worker-side wrapper for Aurora's native NXP GenAVB/TSN AVDECC media-stack shim.
//!
//! This crate is intentionally standalone and platform-gated. It owns no audio
//! rendering or realtime callback behavior. The worker polls the GenAVB control
//! file descriptor, consumes sanitized CONNECT/DISCONNECT events, and leaves
//! ACMP/AVDECC connection ownership inside the pinned GenAVB stack.

use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};

const SHIM_ABI_VERSION: u32 = 1;
const AVDECC_EVENT_CONNECT: u32 = 1;
const AVDECC_EVENT_DISCONNECT: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenAvbAvdeccEventKind {
    Connect,
    Disconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenAvbAvdeccEvent {
    pub kind: GenAvbAvdeccEventKind,
    pub stream_index: u16,
    pub port: u16,
    pub direction: u16,
    pub stream_class: u16,
    pub stream_id: [u8; 8],
    pub destination_mac: [u8; 6],
    pub sample_rate_hz: u32,
    pub channels: u32,
    pub bit_depth: u32,
}

#[derive(Debug)]
pub enum GenAvbAvdeccLoadError {
    InvalidLibraryPath,
    DynamicLibrary(String),
    MissingSymbol(String),
    AbiMismatch { expected: u32, actual: u32 },
    CreateFailed,
}

impl fmt::Display for GenAvbAvdeccLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPath => formatter.write_str("invalid GenAVB shim library path"),
            Self::DynamicLibrary(message) => write!(formatter, "GenAVB shim load error: {message}"),
            Self::MissingSymbol(name) => write!(formatter, "GenAVB shim missing symbol {name}"),
            Self::AbiMismatch { expected, actual } => write!(
                formatter,
                "GenAVB shim ABI mismatch: expected {expected}, got {actual}"
            ),
            Self::CreateFailed => {
                formatter.write_str("GenAVB AVDECC shim failed to create a handle")
            }
        }
    }
}

impl std::error::Error for GenAvbAvdeccLoadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenAvbAvdeccError {
    Native(i32),
    UnknownEventKind(u32),
}

impl fmt::Display for GenAvbAvdeccError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(code) => write!(formatter, "GenAVB AVDECC native error {code}"),
            Self::UnknownEventKind(kind) => {
                write!(formatter, "unknown GenAVB AVDECC event kind {kind}")
            }
        }
    }
}

impl std::error::Error for GenAvbAvdeccError {}

#[repr(C)]
#[derive(Clone, Copy)]
struct ShimAvdeccEvent {
    kind: u32,
    stream_index: u16,
    port: u16,
    direction: u16,
    stream_class: u16,
    stream_id: [u8; 8],
    destination_mac: [u8; 6],
    reserved: [u8; 2],
    sample_rate_hz: u32,
    channels: u32,
    bit_depth: u32,
}

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
type OpenFn = unsafe extern "C" fn(*mut c_void) -> i32;
type RxFdFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ReceiveFn = unsafe extern "C" fn(*mut c_void, *mut ShimAvdeccEvent) -> i32;
type CloseFn = unsafe extern "C" fn(*mut c_void) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct ShimApi {
    open: OpenFn,
    rx_fd: RxFdFn,
    receive: ReceiveFn,
    close: CloseFn,
    destroy: DestroyFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Loaded,
    Open,
}

/// Worker-owned AVDECC media-stack control channel.
///
/// `poll_fd()` is intended for a worker `poll`/`select` loop. `receive()` must
/// only be called after the descriptor indicates readability. No method on this
/// type belongs in Aurora's realtime audio callback.
pub struct GenAvbAvdeccControl {
    library: SharedLibrary,
    api: ShimApi,
    handle: *mut c_void,
    shim_path: PathBuf,
    lifecycle: Lifecycle,
}

unsafe impl Send for GenAvbAvdeccControl {}

impl GenAvbAvdeccControl {
    pub fn load(shim_path: impl Into<PathBuf>) -> Result<Self, GenAvbAvdeccLoadError> {
        let shim_path = shim_path.into();
        let library = SharedLibrary::open(&shim_path)?;
        let abi_version: AbiVersionFn = unsafe { library.symbol(b"aurora_genavb_abi_version\0")? };
        let actual = unsafe { abi_version() };
        if actual != SHIM_ABI_VERSION {
            return Err(GenAvbAvdeccLoadError::AbiMismatch {
                expected: SHIM_ABI_VERSION,
                actual,
            });
        }

        let create: CreateFn = unsafe { library.symbol(b"aurora_genavb_avdecc_create\0")? };
        let api = ShimApi {
            open: unsafe { library.symbol(b"aurora_genavb_avdecc_open\0")? },
            rx_fd: unsafe { library.symbol(b"aurora_genavb_avdecc_rx_fd\0")? },
            receive: unsafe { library.symbol(b"aurora_genavb_avdecc_receive\0")? },
            close: unsafe { library.symbol(b"aurora_genavb_avdecc_close\0")? },
            destroy: unsafe { library.symbol(b"aurora_genavb_avdecc_destroy\0")? },
        };
        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(GenAvbAvdeccLoadError::CreateFailed);
        }

        Ok(Self {
            library,
            api,
            handle,
            shim_path,
            lifecycle: Lifecycle::Loaded,
        })
    }

    pub fn open(&mut self) -> Result<(), GenAvbAvdeccError> {
        if self.lifecycle == Lifecycle::Open {
            return Ok(());
        }
        native_ok(unsafe { (self.api.open)(self.handle) })?;
        self.lifecycle = Lifecycle::Open;
        Ok(())
    }

    pub fn poll_fd(&self) -> Result<i32, GenAvbAvdeccError> {
        if self.lifecycle != Lifecycle::Open {
            return Err(GenAvbAvdeccError::Native(-1));
        }
        let fd = unsafe { (self.api.rx_fd)(self.handle) };
        if fd < 0 {
            Err(GenAvbAvdeccError::Native(fd))
        } else {
            Ok(fd)
        }
    }

    /// Reads one control indication after the worker has observed readiness.
    ///
    /// `Ok(None)` means the native adapter consumed a non-audio AVDECC message
    /// such as BIND/UNBIND/persistent state. Only CONNECT/DISCONNECT are exposed
    /// as audio-stream lifecycle events here.
    pub fn receive(&mut self) -> Result<Option<GenAvbAvdeccEvent>, GenAvbAvdeccError> {
        if self.lifecycle != Lifecycle::Open {
            return Err(GenAvbAvdeccError::Native(-1));
        }
        let mut raw = ShimAvdeccEvent {
            kind: 0,
            stream_index: 0,
            port: 0,
            direction: 0,
            stream_class: 0,
            stream_id: [0; 8],
            destination_mac: [0; 6],
            reserved: [0; 2],
            sample_rate_hz: 0,
            channels: 0,
            bit_depth: 0,
        };
        let rc = unsafe { (self.api.receive)(self.handle, &mut raw) };
        if rc < 0 {
            return Err(GenAvbAvdeccError::Native(rc));
        }
        if rc == 0 {
            return Ok(None);
        }
        decode_event(raw).map(Some)
    }

    pub fn close(&mut self) -> Result<(), GenAvbAvdeccError> {
        if self.lifecycle == Lifecycle::Loaded {
            return Ok(());
        }
        native_ok(unsafe { (self.api.close)(self.handle) })?;
        self.lifecycle = Lifecycle::Loaded;
        Ok(())
    }

    pub fn shim_path(&self) -> &Path {
        &self.shim_path
    }

    /// Opaque native handle for the sibling GenAVB talker adapter. The pointer
    /// is stable only while this control object is alive and open.
    pub fn native_handle(&self) -> Result<*mut c_void, GenAvbAvdeccError> {
        if self.lifecycle != Lifecycle::Open {
            return Err(GenAvbAvdeccError::Native(-1));
        }
        Ok(self.handle)
    }
}

impl Drop for GenAvbAvdeccControl {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            if self.lifecycle == Lifecycle::Open {
                let _ = unsafe { (self.api.close)(self.handle) };
            }
            unsafe { (self.api.destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        let _ = &self.library;
    }
}

fn native_ok(code: i32) -> Result<(), GenAvbAvdeccError> {
    if code == 0 {
        Ok(())
    } else {
        Err(GenAvbAvdeccError::Native(code))
    }
}

fn decode_event(raw: ShimAvdeccEvent) -> Result<GenAvbAvdeccEvent, GenAvbAvdeccError> {
    let kind = match raw.kind {
        AVDECC_EVENT_CONNECT => GenAvbAvdeccEventKind::Connect,
        AVDECC_EVENT_DISCONNECT => GenAvbAvdeccEventKind::Disconnect,
        other => return Err(GenAvbAvdeccError::UnknownEventKind(other)),
    };
    Ok(GenAvbAvdeccEvent {
        kind,
        stream_index: raw.stream_index,
        port: raw.port,
        direction: raw.direction,
        stream_class: raw.stream_class,
        stream_id: raw.stream_id,
        destination_mac: raw.destination_mac,
        sample_rate_hz: raw.sample_rate_hz,
        channels: raw.channels,
        bit_depth: raw.bit_depth,
    })
}

struct SharedLibrary {
    handle: *mut c_void,
}

unsafe impl Send for SharedLibrary {}

impl SharedLibrary {
    fn open(path: &Path) -> Result<Self, GenAvbAvdeccLoadError> {
        platform::open(path).map(|handle| Self { handle })
    }

    unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, GenAvbAvdeccLoadError> {
        let c_name = CStr::from_bytes_with_nul(name)
            .map_err(|_| GenAvbAvdeccLoadError::MissingSymbol("invalid-symbol-name".into()))?;
        let pointer = platform::symbol(self.handle, c_name)?;
        if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err(GenAvbAvdeccLoadError::DynamicLibrary(
                "function pointer size does not match platform pointer size".into(),
            ));
        }
        Ok(std::mem::transmute_copy(&pointer))
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

    pub fn open(path: &Path) -> Result<*mut c_void, GenAvbAvdeccLoadError> {
        let text = path
            .to_str()
            .ok_or(GenAvbAvdeccLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| GenAvbAvdeccLoadError::InvalidLibraryPath)?;
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(GenAvbAvdeccLoadError::DynamicLibrary(last_error()));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, GenAvbAvdeccLoadError> {
        let _ = dlerror();
        let pointer = dlsym(handle, name.as_ptr());
        let error = dlerror();
        if !error.is_null() {
            return Err(GenAvbAvdeccLoadError::MissingSymbol(
                name.to_string_lossy().into_owned(),
            ));
        }
        Ok(pointer)
    }

    pub unsafe fn close(handle: *mut c_void) {
        let _ = dlclose(handle);
    }

    fn last_error() -> String {
        let error = unsafe { dlerror() };
        if error.is_null() {
            return "unknown dynamic-loader error".into();
        }
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(unix))]
mod platform {
    use super::*;

    pub fn open(_path: &Path) -> Result<*mut c_void, GenAvbAvdeccLoadError> {
        Err(GenAvbAvdeccLoadError::DynamicLibrary(
            "GenAVB AVDECC adapter is Linux/Unix platform-gated".into(),
        ))
    }

    pub unsafe fn symbol(
        _handle: *mut c_void,
        _name: &CStr,
    ) -> Result<*mut c_void, GenAvbAvdeccLoadError> {
        Err(GenAvbAvdeccLoadError::DynamicLibrary(
            "GenAVB AVDECC adapter is Linux/Unix platform-gated".into(),
        ))
    }

    pub unsafe fn close(_handle: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(kind: u32) -> ShimAvdeccEvent {
        ShimAvdeccEvent {
            kind,
            stream_index: 5,
            port: 1,
            direction: 0,
            stream_class: 1,
            stream_id: [1, 2, 3, 4, 5, 6, 7, 8],
            destination_mac: [0x91, 0xe0, 0xf0, 0, 0, 5],
            reserved: [0; 2],
            sample_rate_hz: 48_000,
            channels: 2,
            bit_depth: 24,
        }
    }

    #[test]
    fn connect_event_decodes_without_losing_identity() {
        let event = decode_event(raw(AVDECC_EVENT_CONNECT)).unwrap();
        assert_eq!(event.kind, GenAvbAvdeccEventKind::Connect);
        assert_eq!(event.stream_index, 5);
        assert_eq!(event.stream_id, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(event.destination_mac, [0x91, 0xe0, 0xf0, 0, 0, 5]);
        assert_eq!(event.sample_rate_hz, 48_000);
        assert_eq!(event.channels, 2);
        assert_eq!(event.bit_depth, 24);
    }

    #[test]
    fn disconnect_event_is_distinct() {
        let event = decode_event(raw(AVDECC_EVENT_DISCONNECT)).unwrap();
        assert_eq!(event.kind, GenAvbAvdeccEventKind::Disconnect);
    }

    #[test]
    fn unknown_event_kind_fails_closed() {
        assert_eq!(
            decode_event(raw(99)),
            Err(GenAvbAvdeccError::UnknownEventKind(99))
        );
    }
}
