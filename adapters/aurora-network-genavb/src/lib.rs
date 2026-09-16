//! Platform-gated NXP GenAVB/TSN AAF talker behind Aurora's worker contract.
//!
//! The adapter is intentionally outside Aurora's default workspace. It loads an
//! Aurora-owned native shim compiled against the exact pinned GenAVB/TSN API on
//! supported Linux/NXP deployments. Network and GenAVB calls remain on the
//! worker side; the realtime callback only publishes bounded PCM blocks through
//! Aurora's existing callback -> worker bridge.

use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};

use aurora_network_genavb_avdecc::GenAvbAvdeccControl;
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTransportCapabilities,
    NetworkTransportError, NetworkTransportEvent, NetworkTransportFamily,
    AURORA_NETWORK_MEDIA_RATE,
};

const SHIM_ABI_VERSION: u32 = 1;
const GENAVB_CHANNELS: usize = 2;
const GENAVB_BLOCK_FRAMES: usize = 48;
const AAF_BYTES_PER_SAMPLE: usize = 4;
const EVENT_CAPACITY: usize = 8;

/// Static parameters for one GenAVB AAF talker stream.
///
/// This configuration is retained for the explicit static/manual validation
/// path. Normal AVDECC operation uses [`GenAvbNetworkTransport::load_avdecc`]
/// and receives stream identity from the GenAVB media-stack CONNECT state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenAvbTalkerConfig {
    /// Path to Aurora's native GenAVB shim shared library.
    pub shim_path: PathBuf,
    /// GenAVB network port index.
    pub port: u16,
    /// Eight-byte AVTP stream identifier in network byte order.
    pub stream_id: [u8; 8],
    /// Six-byte multicast destination MAC address.
    pub destination_mac: [u8; 6],
}

/// Native shim load/configuration failures.
#[derive(Debug)]
pub enum GenAvbAdapterLoadError {
    InvalidLibraryPath,
    DynamicLibrary(String),
    MissingSymbol(String),
    AbiMismatch { expected: u32, actual: u32 },
    CreateFailed,
}

impl fmt::Display for GenAvbAdapterLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPath => formatter.write_str("invalid GenAVB shim library path"),
            Self::DynamicLibrary(message) => write!(formatter, "GenAVB shim load error: {message}"),
            Self::MissingSymbol(name) => write!(formatter, "GenAVB shim missing symbol {name}"),
            Self::AbiMismatch { expected, actual } => write!(
                formatter,
                "GenAVB shim ABI mismatch: expected {expected}, got {actual}"
            ),
            Self::CreateFailed => formatter.write_str("GenAVB shim failed to create a handle"),
        }
    }
}

impl std::error::Error for GenAvbAdapterLoadError {}

#[repr(C)]
struct ShimConfig {
    port: u16,
    _reserved: u16,
    stream_id: [u8; 8],
    destination_mac: [u8; 6],
    _padding: [u8; 2],
    target_latency_frames: u32,
    block_frames: u32,
}

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
type PrepareFn = unsafe extern "C" fn(*mut c_void, *const ShimConfig) -> i32;
type PrepareAvdeccFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u16, u32, u32) -> i32;
type StartFn = unsafe extern "C" fn(*mut c_void) -> i32;
type SubmitFn = unsafe extern "C" fn(*mut c_void, *const u8, u32, u64) -> i32;
type StopFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ResetFn = unsafe extern "C" fn(*mut c_void) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct ShimApi {
    prepare: PrepareFn,
    prepare_avdecc: PrepareAvdeccFn,
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

/// One GenAVB/TSN AAF stereo talker implementing Aurora's worker-side contract.
pub struct GenAvbNetworkTransport {
    library: SharedLibrary,
    api: ShimApi,
    handle: *mut c_void,
    shim_path: PathBuf,
    static_config: Option<GenAvbTalkerConfig>,
    lifecycle: Lifecycle,
    prepared_format: Option<NetworkAudioFormat>,
    expected_sequence: Option<u64>,
    expected_timestamp: Option<MediaTimestamp>,
    packed_aaf: Vec<u8>,
    events: [Option<NetworkTransportEvent>; EVENT_CAPACITY],
    event_read: usize,
    event_write: usize,
    event_count: usize,
}

// Native ownership is confined to the network worker. No native call belongs in
// Aurora's realtime audio callback.
unsafe impl Send for GenAvbNetworkTransport {}

impl GenAvbNetworkTransport {
    /// Loads the Aurora-owned native shim for the static/manual stream path.
    pub fn load(config: GenAvbTalkerConfig) -> Result<Self, GenAvbAdapterLoadError> {
        let shim_path = config.shim_path.clone();
        Self::load_inner(shim_path, Some(config))
    }

    /// Loads a talker whose network identity is supplied by AVDECC CONNECT.
    ///
    /// No placeholder stream ID, multicast MAC or port is accepted here. The
    /// stream can only be prepared through [`Self::prepare_from_avdecc`].
    pub fn load_avdecc(shim_path: impl Into<PathBuf>) -> Result<Self, GenAvbAdapterLoadError> {
        Self::load_inner(shim_path.into(), None)
    }

    fn load_inner(
        shim_path: PathBuf,
        static_config: Option<GenAvbTalkerConfig>,
    ) -> Result<Self, GenAvbAdapterLoadError> {
        let library = SharedLibrary::open(&shim_path)?;
        let abi_version: AbiVersionFn = unsafe { library.symbol(b"aurora_genavb_abi_version\0")? };
        let actual = unsafe { abi_version() };
        if actual != SHIM_ABI_VERSION {
            return Err(GenAvbAdapterLoadError::AbiMismatch {
                expected: SHIM_ABI_VERSION,
                actual,
            });
        }

        let create: CreateFn = unsafe { library.symbol(b"aurora_genavb_create\0")? };
        let api = ShimApi {
            prepare: unsafe { library.symbol(b"aurora_genavb_prepare\0")? },
            prepare_avdecc: unsafe { library.symbol(b"aurora_genavb_prepare_avdecc\0")? },
            start: unsafe { library.symbol(b"aurora_genavb_start\0")? },
            submit: unsafe { library.symbol(b"aurora_genavb_submit\0")? },
            stop: unsafe { library.symbol(b"aurora_genavb_stop\0")? },
            reset: unsafe { library.symbol(b"aurora_genavb_reset\0")? },
            destroy: unsafe { library.symbol(b"aurora_genavb_destroy\0")? },
        };
        let handle = unsafe { create() };
        if handle.is_null() {
            return Err(GenAvbAdapterLoadError::CreateFailed);
        }

        Ok(Self {
            library,
            api,
            handle,
            shim_path,
            static_config,
            lifecycle: Lifecycle::Loaded,
            prepared_format: None,
            expected_sequence: None,
            expected_timestamp: None,
            packed_aaf: Vec::new(),
            events: [None; EVENT_CAPACITY],
            event_read: 0,
            event_write: 0,
            event_count: 0,
        })
    }

    /// Prepares this talker from the exact stream parameters cached by the
    /// sibling AVDECC control adapter after a supported CONNECT indication.
    pub fn prepare_from_avdecc(
        &mut self,
        control: &GenAvbAvdeccControl,
        stream_index: u16,
        config: NetworkStreamConfig,
    ) -> Result<(), NetworkTransportError> {
        if self.lifecycle == Lifecycle::Started {
            return Err(NetworkTransportError::AlreadyStarted);
        }
        if self.shim_path != control.shim_path() {
            return Err(NetworkTransportError::WorkerFault);
        }
        let samples = self.validate_prepare_config(config)?;
        let control_handle = control
            .native_handle()
            .map_err(|_| NetworkTransportError::WorkerFault)?;
        Self::native_ok(unsafe {
            (self.api.prepare_avdecc)(
                self.handle,
                control_handle,
                stream_index,
                config.timing.target_latency_frames,
                GENAVB_BLOCK_FRAMES as u32,
            )
        })?;
        self.commit_prepared(config, samples);
        Ok(())
    }

    pub fn shim_path(&self) -> &Path {
        &self.shim_path
    }

    fn validate_prepare_config(
        &self,
        config: NetworkStreamConfig,
    ) -> Result<usize, NetworkTransportError> {
        config.format.validate(self.capabilities())?;
        config.timing.validate()?;
        if config.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
            || config.format.channels != GENAVB_CHANNELS
            || config.format.block_frames != GENAVB_BLOCK_FRAMES
            || config.clock_discipline != NetworkClockDiscipline::PtpFollower
            || config.timing.minimum_latency_frames != config.timing.target_latency_frames
            || config.timing.maximum_latency_frames != config.timing.target_latency_frames
            || config.timing.maximum_rate_correction_ppm != 0.0
        {
            return Err(NetworkTransportError::InvalidFormat);
        }
        config
            .format
            .samples_per_block()
            .ok_or(NetworkTransportError::InvalidFormat)
    }

    fn commit_prepared(&mut self, config: NetworkStreamConfig, samples: usize) {
        self.packed_aaf.resize(samples * AAF_BYTES_PER_SAMPLE, 0);
        self.prepared_format = Some(config.format);
        self.lifecycle = Lifecycle::Prepared;
        self.reset_timeline();
        self.push_event(NetworkTransportEvent::Prepared);
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

    fn reset_timeline(&mut self) {
        self.expected_sequence = None;
        self.expected_timestamp = None;
    }

    fn native_ok(code: i32) -> Result<(), NetworkTransportError> {
        if code == 0 {
            Ok(())
        } else {
            Err(NetworkTransportError::WorkerFault)
        }
    }
}

impl NetworkAudioTransport for GenAvbNetworkTransport {
    fn capabilities(&self) -> NetworkTransportCapabilities {
        NetworkTransportCapabilities {
            family: NetworkTransportFamily::AvbTsn,
            max_channels: GENAVB_CHANNELS,
            scheduled_playout: true,
            // Physical timestamp quality is platform evidence, not a generic
            // software-adapter claim.
            hardware_timestamps: false,
            adaptive_rate_matching: false,
            packet_repair: false,
        }
    }

    fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
        if self.lifecycle == Lifecycle::Started {
            return Err(NetworkTransportError::AlreadyStarted);
        }
        let static_config = self
            .static_config
            .as_ref()
            .ok_or(NetworkTransportError::WorkerFault)?;
        let samples = self.validate_prepare_config(config)?;

        let native = ShimConfig {
            port: static_config.port,
            _reserved: 0,
            stream_id: static_config.stream_id,
            destination_mac: static_config.destination_mac,
            _padding: [0; 2],
            target_latency_frames: config.timing.target_latency_frames,
            block_frames: GENAVB_BLOCK_FRAMES as u32,
        };
        Self::native_ok(unsafe { (self.api.prepare)(self.handle, &native) })?;
        self.commit_prepared(config, samples);
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
        if self
            .expected_sequence
            .is_some_and(|expected| block.sequence != expected)
            || self
                .expected_timestamp
                .is_some_and(|expected| block.timestamp != expected)
        {
            return Err(NetworkTransportError::WorkerFault);
        }

        pack_f32_to_aaf_s24_in_s32_be(block.samples, &mut self.packed_aaf)?;
        let bytes = u32::try_from(self.packed_aaf.len())
            .map_err(|_| NetworkTransportError::InvalidFormat)?;
        Self::native_ok(unsafe {
            (self.api.submit)(
                self.handle,
                self.packed_aaf.as_ptr(),
                bytes,
                block.timestamp.frame_index,
            )
        })?;

        self.expected_sequence = Some(
            block
                .sequence
                .checked_add(1)
                .ok_or(NetworkTransportError::WorkerFault)?,
        );
        self.expected_timestamp = Some(block.timestamp.checked_advance(block.format.block_frames)?);
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
        if self.lifecycle == Lifecycle::Started {
            let _ = unsafe { (self.api.stop)(self.handle) };
            self.lifecycle = Lifecycle::Prepared;
        }
        if self.lifecycle != Lifecycle::Loaded {
            let _ = unsafe { (self.api.reset)(self.handle) };
        }
        self.reset_timeline();
    }
}

impl Drop for GenAvbNetworkTransport {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        let _ = &self.library;
    }
}

/// Converts interleaved Aurora f32 PCM into AAF INT32 payload bytes with
/// 24 valid MSBs and eight zero padding LSBs, serialized big-endian.
fn pack_f32_to_aaf_s24_in_s32_be(
    samples: &[f32],
    output: &mut [u8],
) -> Result<(), NetworkTransportError> {
    if output.len() != samples.len() * AAF_BYTES_PER_SAMPLE {
        return Err(NetworkTransportError::BufferShape);
    }
    for (sample, bytes) in samples.iter().zip(output.chunks_exact_mut(4)) {
        if !sample.is_finite() {
            return Err(NetworkTransportError::WorkerFault);
        }
        let scaled = (*sample as f64 * 8_388_608.0).round();
        let pcm24 = scaled.clamp(-8_388_608.0, 8_388_607.0) as i32;
        let word = pcm24 << 8;
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    Ok(())
}

struct SharedLibrary {
    handle: *mut c_void,
}

unsafe impl Send for SharedLibrary {}

impl SharedLibrary {
    fn open(path: &Path) -> Result<Self, GenAvbAdapterLoadError> {
        platform::open(path).map(|handle| Self { handle })
    }

    unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, GenAvbAdapterLoadError> {
        let c_name = CStr::from_bytes_with_nul(name)
            .map_err(|_| GenAvbAdapterLoadError::MissingSymbol("invalid-symbol-name".into()))?;
        let ptr = platform::symbol(self.handle, c_name)?;
        if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err(GenAvbAdapterLoadError::DynamicLibrary(
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

    pub fn open(path: &Path) -> Result<*mut c_void, GenAvbAdapterLoadError> {
        let text = path
            .to_str()
            .ok_or(GenAvbAdapterLoadError::InvalidLibraryPath)?;
        let path = CString::new(text).map_err(|_| GenAvbAdapterLoadError::InvalidLibraryPath)?;
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(GenAvbAdapterLoadError::DynamicLibrary(last_error()));
        }
        Ok(handle)
    }

    pub unsafe fn symbol(
        handle: *mut c_void,
        name: &CStr,
    ) -> Result<*mut c_void, GenAvbAdapterLoadError> {
        let _ = dlerror();
        let pointer = dlsym(handle, name.as_ptr());
        let error = dlerror();
        if !error.is_null() {
            return Err(GenAvbAdapterLoadError::MissingSymbol(
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

    pub fn open(_path: &Path) -> Result<*mut c_void, GenAvbAdapterLoadError> {
        Err(GenAvbAdapterLoadError::DynamicLibrary(
            "GenAVB talker adapter is Linux/Unix platform-gated".into(),
        ))
    }

    pub unsafe fn symbol(
        _handle: *mut c_void,
        _name: &CStr,
    ) -> Result<*mut c_void, GenAvbAdapterLoadError> {
        Err(GenAvbAdapterLoadError::DynamicLibrary(
            "GenAVB talker adapter is Linux/Unix platform-gated".into(),
        ))
    }

    pub unsafe fn close(_handle: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aaf_s24_in_s32_big_endian_conversion_is_saturating_and_deterministic() {
        let samples = [-1.0_f32, -0.5, 0.0, 0.5, 1.0, 2.0];
        let mut bytes = vec![0_u8; samples.len() * 4];
        pack_f32_to_aaf_s24_in_s32_be(&samples, &mut bytes).unwrap();
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[4..8], &[0xC0, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[8..12], &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[12..16], &[0x40, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[16..20], &[0x7F, 0xFF, 0xFF, 0x00]);
        assert_eq!(&bytes[20..24], &[0x7F, 0xFF, 0xFF, 0x00]);
    }

    #[test]
    fn non_finite_pcm_fails_closed() {
        let mut bytes = [0_u8; 4];
        assert_eq!(
            pack_f32_to_aaf_s24_in_s32_be(&[f32::NAN], &mut bytes),
            Err(NetworkTransportError::WorkerFault)
        );
    }

    #[test]
    fn incorrect_output_shape_fails_closed() {
        let mut bytes = [0_u8; 3];
        assert_eq!(
            pack_f32_to_aaf_s24_in_s32_be(&[0.0], &mut bytes),
            Err(NetworkTransportError::BufferShape)
        );
    }
}
