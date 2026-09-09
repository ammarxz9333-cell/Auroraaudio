//! CPAL-backed local audio backend for Aurora.
//!
//! Linux direct-eARC builds deliberately use Aurora's native ALSA/ASoC
//! capture/output crates instead of CPAL. This keeps the Linux dependency graph
//! on one ALSA generation and prevents duplicate `links = "asound"` packages.

#[cfg(not(target_os = "linux"))]
mod cpal_impl;
#[cfg(not(target_os = "linux"))]
pub use cpal_impl::CpalAudioBackend;

#[cfg(target_os = "linux")]
mod linux_disabled {
    use aurora_realtime_audio_api::{
        AudioDeviceInfo, AudioInputBackend, AudioInputStream, AudioOutputBackend,
        AudioOutputStream, InputCallback, OutputCallback, RealTimeAudioConfig,
        RealTimeAudioError,
    };

    /// Compatibility type retained so workspace/default-feature Linux builds
    /// still compile. The product eARC path uses native ALSA/ASoC instead.
    #[derive(Debug, Default, Clone)]
    pub struct CpalAudioBackend;

    impl CpalAudioBackend {
        pub fn new() -> Self {
            Self
        }

        fn disabled() -> RealTimeAudioError {
            RealTimeAudioError::UnsupportedFormat(
                "CPAL backend is disabled on Linux; use Aurora native ALSA/ASoC I/O".to_owned(),
            )
        }
    }

    impl AudioInputBackend for CpalAudioBackend {
        fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
            Err(Self::disabled())
        }

        fn open_input(
            &self,
            _config: &RealTimeAudioConfig,
            _callback: InputCallback,
        ) -> Result<Box<dyn AudioInputStream>, RealTimeAudioError> {
            Err(Self::disabled())
        }

        fn device_reported_latency_frames(&self) -> Option<usize> {
            None
        }
    }

    impl AudioOutputBackend for CpalAudioBackend {
        fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
            Err(Self::disabled())
        }

        fn open_output(
            &self,
            _config: &RealTimeAudioConfig,
            _callback: OutputCallback,
        ) -> Result<Box<dyn AudioOutputStream>, RealTimeAudioError> {
            Err(Self::disabled())
        }

        fn device_reported_latency_frames(&self) -> Option<usize> {
            None
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux_disabled::CpalAudioBackend;
