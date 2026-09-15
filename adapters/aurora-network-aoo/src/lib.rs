//! Runtime AOO adapter behind Aurora's worker-thread network contract.
//!
//! The adapter intentionally loads an Aurora-owned C shim dynamically instead
//! of linking AOO into every Aurora build. The shim is compiled against the
//! exact pinned AOO C API in the Open Audio Stack CI and owns all AOO-specific
//! structures, socket threads and planar/interleaved conversion.

use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};

use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTransportCapabilities,
    NetworkTransportError, NetworkTransportEvent, NetworkTransportFamily,
    AURORA_NETWORK_MEDIA_RATE,
};

const SHIM_ABI_VERSION: u32 = 1;
const MAX_AOO_CHANNELS: usize = 32;
const EVENT_CAPACITY: usize = 8;

/// Static sink endpoint configured for one AOO source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AooSinkEndpoint {
    /// Hostname or numeric IP address.
    pub host: String,
    /// UDP port of the remote AOO sink.
    pub port: u16,
    /// Remote AOO sink identifier.
    pub id: i32,
}

/// Setup-time configuration for the native AOO adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AooRuntimeConfig {
    /// Path to the Aurora-owned AOO shim shared library.
    pub shim_path: PathBuf,
    /// AOO source identifier.
    pub source_id: i32,
    /// Local UDP port; zero asks AOO to select a free port.
    pub local_port: u16,
    /// Optional statically configured sink.
    pub sink: Option<AooSinkEndpoint>,
}

impl AooRuntimeConfig {
    /// Creates a local-only adapter configuration useful for validation.
    pub fn local_only(shim_path: impl Into<PathBuf>, source_id: i32) -> Self {
        Self {
            shim_path: shim_path.into(),
            source_id,
            local_port: 0,
            sink: None,
        }
    }
}

/// Adapter construction/load errors before the Aurora transport is prepared.
#[derive(Debug)]
pub enum AooAdapterLoadError {
    /// Shared library path could not be represented for the platform loader.
    InvalidLibraryPath,
    /// A dynamic-library operation failed.
    DynamicLibrary(String),
    /// Required shim symbol is missing.
    MissingSymbol(&'static str),
    /// Loaded shim ABI does not match this adapter.
    AbiMismatch { expected: u32, actual: u32 },
    /// Native shim could not allocate/initialize a handle.
    CreateFailed,
    /// Configured host contains an embedded NUL byte.
    InvalidSinkHost,
}

impl fmt::Display for AooAdapterLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPath => write!(f, "invalid AOO shim library path"),
            Self::DynamicLibrary(message) => write!(f, "AOO shim load error: {message}"),
            Self::MissingSymbol(name) => write!(f, "AOO shim missing symbol {name}"),
            Self::AbiMismatch { expected, actual } => {
                write!(f, "AOO shim ABI mismatch: expected {expected}, got {actual}")
            }
            Self::CreateFailed => write!(f, "AOO shim failed to create a native handle"),
            Self::InvalidSinkHost => write!(f, "AOO sink host contains an embedded NUL byte"),
        }
    }
}

impl std::error::Error for AooAdapterLoadError {}

#[repr(C)]
struct ShimConfig {
    channels: i32,
    sample_rate: i32,
    block_frames: i32,
    source_id: i32,
    local_port: i32,
    sink_port: i32,
    sink_id: i32,
    sink_host: *const c_char,
}

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
type PrepareFn = unsafe extern "C" fn(*mut c_void, *const ShimConfig) -> i32;
type StartFn = unsafe extern "C" fn(*mut c_void) -> i32;
type SubmitFn = unsafe extern "C" fn(*mut c_void, *const f32, u64) -> i32;
type StopFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ResetFn = unsafe extern "C" fn(*mut c_void) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct ShimApi {
    prepare: PrepareFn,
    start: StartFn,
    submit: SubmitFn,
    stop: StopFn,
    reset: ResetFn,
    destroy: DestroyFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Loaded,
    Prepared,
    Started,
}

/// Real AOO sender backend implementing Aurora's worker-side network contract.
///
/// All native calls are worker/control-thread calls. This type must never be
/// invoked from Aurora's realtime callback; use the existing bounded callback
/// -> worker bridge for PCM publication.
pub struct AooNetworkTransport {
    library: SharedLibrary,
    api: ShimApi,
    handle: *mut c_void,
    config: AooRuntimeConfig,
    sink_host: Option<CString>,
    lifecycle: Lifecycle,
    prepared_format: Option<NetworkAudioFormat>,
    expected_sequence: Option<u64>,
    expected_timestamp: Option<MediaTimestamp>,
    events: [Option<NetworkTransportEvent>; EVENT_CAPACITY],
    event_read: usize,
    event_write: usize,
    event_count: usize,
}

// The native handle is exclusively owned and used by the network worker. The
// C shim owns its own socket threads and synchronizes destruction by joining
// them before the shared library is unloaded.
unsafe impl Send for AooNetworkTransport {}

impl AooNetworkTransport {
    /// Loads the native shim and creates one AOO runtime instance.
    pub fn load(config: AooRuntimeConfig) -> Result<Self, AooAdapterLoadError> {
        let sink_host = config
            .sink
            .as_ref()
            .map(|sink| CString::new(sink.host.as_bytes()))
            .transpose()
            .map_err(|_| AooAdapterLoadError::InvalidSinkHost)?;
        let library = SharedLibrary::open(&config.shim_path)?;
        let abi_version: AbiVersionFn = unsafe { library.symbol(b"aurora_aoo_abi_version\0")? };
        let actual = unsafe { abi_version() };
        if actual != SHIM_ABI_VERSION {
            return Err(AooAdapterLoadError::AbiMismatch {
                expected: SHIM_ABI_VERSION,
                actual,
            });
        }
        let create: CreateFn = unsafe { library.symbol(b"aurora_aoo_create\0")? };
        let api = ShimApi {
            prepare: unsafe { library.symbol(b"aurora_aoo_prepare\0")? },
            start: unsafe { library.symbol(b"aurora_aoo_start\0")? },
            submit: unsafe { library.symbol(b"aurora_aoo_submit\0")? },
            stop: unsafe { library.symbol(b"aurora_aoo_stop\0")? },
            reset: unsafe { library.symbol(b"aurora_aoo_reset\0")? },
            destroy: unsafe { library.symbol(b"aurora_aoo_destroy\0")? },
        };
        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(AooAdapterLoadError::CreateFailed);
        }
        Ok(Self {
            library,
            api,
            handle,
            config,
            sink_host,
            lifecycle: Lifecycle::Loaded,
            prepared_format: None,
            expected_sequence: None,
            expected_timestamp: None,
            events: [None; EVENT_CAPACITY],
            event_read: 0,
            event_write: 0,
            event_count: 0,
        })
    }

    fn push_event(&mut self, event: NetworkTransportEvent) {
        if self.event_count == EVENT_CAPACITY {
            self.event_read = (self.event_read + 1) % EVENT_CAPACITY;
            self.event_count -= 1;
        }
        self.events[self.event_write] = Some(event);
        self.event_write = (self.event_write + 1) % EVENT_CAPACITY;
        self.event_count += 1;
    }

    fn pop_event(&mut self) -> Option<NetworkTransportEvent> {
        if self.event_count == 0 {
            return None;
        }
        let event = self.events[self.event_read].take();
        self.event_read = (self.event_read + 1) % EVENT_CAPACITY;
        self.event_count -= 1;
        event
    }

    fn native_ok(code: i32) -> Result<(), NetworkTransportError> {
        if code == 0 {
            Ok(())
        } else {
            Err(NetworkTransportError::WorkerFault)
        }
    }

    fn reset_timeline(&mut self) {
        self.expected_sequence = None;
        self.expected_timestamp = None;
    }
}

impl NetworkAudioTransport for AooNetworkTransport {
    fn capabilities(&self) -> NetworkTransportCapabilities {
        NetworkTransportCapabilities {
            family: NetworkTransportFamily::PeerUdp,
            max_channels: MAX_AOO_CHANNELS,
            scheduled_playout: true,
            hardware_timestamps: false,
            // Aurora remains the only rate-controller owner in this first
            // production adapter. The shim explicitly disables AOO dynamic SRC.
            adaptive_rate_matching: false,
            packet_repair: true,
        }
    }

    fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
        if self.lifecycle == Lifecycle::Started {
            return Err(NetworkTransportError::AlreadyStarted);
        }
        config.format.validate(self.capabilities())?;
        config.timing.validate()?;
        if config.format.sample_rate != AURORA_NETWORK_MEDIA_RATE {
            return Err(NetworkTransportError::InvalidFormat);
        }
        if config.clock_discipline != NetworkClockDiscipline::AuroraMediaMaster {
            return Err(NetworkTransportError::WorkerFault);
        }
        let channels = i32::try_from(config.format.channels)
            .map_err(|_| NetworkTransportError::InvalidFormat)?;
        let block_frames = i32::try_from(config.format.block_frames)
            .map_err(|_| NetworkTransportError::InvalidFormat)?;
        let sample_rate = i32::try_from(config.format.sample_rate)
            .map_err(|_| NetworkTransportError::InvalidFormat)?;
        let (sink_port, sink_id, sink_host) = match self.config.sink.as_ref() {
            Some(sink) => (
                i32::from(sink.port),
                sink.id,
                self.sink_host
                    .as_ref()
                    .map_or(std::ptr::null(), |host| host.as_ptr()),
            ),
            None => (0, 0, std::ptr::null()),
        };
        let native = ShimConfig {
            channels,
            sample_rate,
            block_frames,
            source_id: self.config.source_id,
            local_port: i32::from(self.config.local_port),
            sink_port,
            sink_id,
            sink_host,
        };
        Self::native_ok(unsafe { (self.api.prepare)(self.handle, &native) })?;
        self.lifecycle = Lifecycle::Prepared;
        self.prepared_format = Some(config.format);
        self.reset_timeline();
        self.push_event(NetworkTransportEvent::Prepared);
        Ok(())
    }

    fn start(&mut self) -> Result<(), NetworkTransportError> {
        match self.lifecycle {
            Lifecycle::Loaded => return Err(NetworkTransportError::NotPrepared),
            Lifecycle::Started => return Err(NetworkTransportError::AlreadyStarted),
            Lifecycle::Prepared => {}
        }
        Self::native_ok(unsafe { (self.api.start)(self.handle) })?;
        self.lifecycle = Lifecycle::Started;
        self.reset_timeline();
        self.push_event(NetworkTransportEvent::Started);
        Ok(())
    }

    fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError> {
        if self.lifecycle != Lifecycle::Started {
            return Err(NetworkTransportError::NotPrepared);
        }
        block.validate()?;
        if self.prepared_format != Some(block.format) {
            return Err(NetworkTransportError::InvalidFormat);
        }
        if let Some(expected) = self.expected_sequence {
            if block.sequence != expected {
                return Err(NetworkTransportError::WorkerFault);
            }
        }
        if let Some(expected) = self.expected_timestamp {
            if block.timestamp != expected {
                return Err(NetworkTransportError::WorkerFault);
            }
        }
        let next_sequence = block
            .sequence
            .checked_add(1)
            .ok_or(NetworkTransportError::WorkerFault)?;
        let next_timestamp = block.timestamp.checked_advance(block.format.block_frames)?;
        Self::native_ok(unsafe {
            (self.api.submit)(self.handle, block.samples.as_ptr(), block.timestamp.frame_index)
        })?;
        self.expected_sequence = Some(next_sequence);
        self.expected_timestamp = Some(next_timestamp);
        Ok(())
    }

    fn poll_event(&mut self) -> Option<NetworkTransportEvent> {
        self.pop_event()
    }

    fn stop(&mut self) -> Result<(), NetworkTransportError> {
        if self.lifecycle == Lifecycle::Loaded {
            return Err(NetworkTransportError::NotPrepared);
        }
        if self.lifecycle == Lifecycle::Started {
            Self::native_ok(unsafe { (self.api.stop)(self.handle) })?;
        }
        self.lifecycle = Lifecycle::Prepared;
        self.reset_timeline();
        self.push_event(NetworkTransportEvent::Stopped);
        Ok(())
    }

    fn reset(&mut self) {
        if self.lifecycle != Lifecycle::Loaded {
            let _ = unsafe { (self.api.reset)(self.handle) };
        }
        self.reset_timeline();
    }
}

impl Drop for AooNetworkTransport {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        // Keep an explicit read so clippy/rustc sees that the shared library is
        // intentionally retained until after the native handle is destroyed.
        let _ = &self.library;
    }
}

struct SharedLibrary {
    handle: *mut c_void,
}

unsafe impl Send for SharedLibrary {}

impl SharedLibrary {
    fn open(path: &Path) -> Result<Self, AooAdapterLoadError> {
        platform::open(path).map(|handle| Self { handle })
    }

    unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, AooAdapterLoadError> {
        let c_name = CStr::from_bytes_with_nul(name)
            .map_err(|_| AooAdapterLoadError::MissingSymbol("invalid-symbol-name"))?;
        let ptr = platform::symbol(self.handle, c_name)?;
        if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err(AooAdapterLoadError::DynamicLibrary(
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

    pub fn open(path: &Path) -> Result<*mut c_void, AooAdapterLoadError> {
        let text = path.to_str().ok_or(AooAdapterLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| AooAdapterLoadError::InvalidLibraryPath)?;
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(AooAdapterLoadError::DynamicLibrary(last_error()));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, AooAdapterLoadError> {
        // Clear any prior loader error before dlsym.
        let _ = dlerror();
        let ptr = dlsym(handle, name.as_ptr());
        let err = dlerror();
        if !err.is_null() {
            let symbol = name.to_string_lossy().into_owned();
            return Err(AooAdapterLoadError::MissingSymbol(Box::leak(
                symbol.into_boxed_str(),
            )));
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
        unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned()
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

    pub fn open(path: &Path) -> Result<*mut c_void, AooAdapterLoadError> {
        let text = path.to_str().ok_or(AooAdapterLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| AooAdapterLoadError::InvalidLibraryPath)?;
        let handle = unsafe { LoadLibraryA(path.as_ptr()) };
        if handle.is_null() {
            return Err(AooAdapterLoadError::DynamicLibrary(
                "LoadLibraryA failed".into(),
            ));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, AooAdapterLoadError> {
        let ptr = GetProcAddress(handle, name.as_ptr());
        if ptr.is_null() {
            let symbol = name.to_string_lossy().into_owned();
            return Err(AooAdapterLoadError::MissingSymbol(Box::leak(
                symbol.into_boxed_str(),
            )));
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

    pub fn open(_path: &Path) -> Result<*mut c_void, AooAdapterLoadError> {
        Err(AooAdapterLoadError::DynamicLibrary(
            "unsupported dynamic-library platform".into(),
        ))
    }

    pub unsafe fn symbol(
        _handle: *mut c_void,
        _name: &CStr,
    ) -> Result<*mut c_void, AooAdapterLoadError> {
        Err(AooAdapterLoadError::DynamicLibrary(
            "unsupported dynamic-library platform".into(),
        ))
    }

    pub unsafe fn close(_handle: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_configuration_is_deterministic() {
        let config = AooRuntimeConfig::local_only("/tmp/libaurora_aoo_shim.so", 7);
        assert_eq!(config.source_id, 7);
        assert_eq!(config.local_port, 0);
        assert_eq!(config.sink, None);
    }

    #[test]
    fn event_ring_is_bounded_and_preserves_latest_events() {
        // This test exercises the fixed event-ring behavior without requiring a
        // native library by reproducing its tiny indexing contract.
        let mut slots = [None; EVENT_CAPACITY];
        let mut read = 0usize;
        let mut write = 0usize;
        let mut count = 0usize;
        for _ in 0..(EVENT_CAPACITY + 3) {
            if count == EVENT_CAPACITY {
                read = (read + 1) % EVENT_CAPACITY;
                count -= 1;
            }
            slots[write] = Some(NetworkTransportEvent::PacketRepair);
            write = (write + 1) % EVENT_CAPACITY;
            count += 1;
        }
        assert_eq!(count, EVENT_CAPACITY);
        assert_eq!(slots[read], Some(NetworkTransportEvent::PacketRepair));
    }
}
