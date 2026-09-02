//! Minimal safe wrapper around the libasound PCM API used by Aurora R1.
//!
//! All direct ALSA `unsafe` code lives in this module. The runtime requests an
//! exact interleaved S32_LE stream and explicitly configures both hardware and
//! software thresholds so start/recovery behaviour is deterministic.

#![cfg(target_os = "linux")]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
use std::ptr;

const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

#[derive(Debug, Clone)]
pub struct AlsaPlaybackConfig {
    pub device: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub period_frames: usize,
    pub buffer_periods: usize,
}

#[derive(Debug)]
pub struct AlsaPlayback {
    pcm: *mut c_void,
    channels: usize,
    period_frames: usize,
    buffer_frames: usize,
    recoveries: u64,
}

impl AlsaPlayback {
    pub fn open(config: &AlsaPlaybackConfig) -> Result<Self, String> {
        if config.channels == 0 {
            return Err("ALSA channel count must be greater than zero".to_owned());
        }
        if config.period_frames == 0 || config.buffer_periods < 2 {
            return Err("invalid ALSA period/buffer configuration".to_owned());
        }

        let device = CString::new(config.device.as_str())
            .map_err(|_| "ALSA device name contains NUL".to_owned())?;
        let mut pcm = ptr::null_mut();
        alsa_check(
            unsafe { snd_pcm_open(&mut pcm, device.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) },
            "snd_pcm_open",
        )?;

        let setup = configure_pcm(pcm, config);
        let (period_frames, buffer_frames) = match setup {
            Ok(values) => values,
            Err(error) => {
                unsafe { snd_pcm_close(pcm) };
                return Err(error);
            }
        };

        Ok(Self {
            pcm,
            channels: config.channels,
            period_frames,
            buffer_frames,
            recoveries: 0,
        })
    }

    pub fn period_frames(&self) -> usize {
        self.period_frames
    }

    pub fn buffer_frames(&self) -> usize {
        self.buffer_frames
    }

    pub fn recoveries(&self) -> u64 {
        self.recoveries
    }

    pub fn delay_frames(&self) -> Result<usize, String> {
        let mut delay: c_long = 0;
        let result = unsafe { snd_pcm_delay(self.pcm, &mut delay) };
        alsa_check(result, "snd_pcm_delay")?;
        Ok(delay.max(0) as usize)
    }

    pub fn write_interleaved(&mut self, samples: &[i32]) -> Result<bool, String> {
        if samples.len() % self.channels != 0 {
            return Err("internal error: ALSA write is not frame-aligned".to_owned());
        }

        let total_frames = samples.len() / self.channels;
        let mut frame_offset = 0usize;
        let mut recovered = false;
        while frame_offset < total_frames {
            let sample_offset = frame_offset * self.channels;
            let remaining = total_frames - frame_offset;
            let result = unsafe {
                snd_pcm_writei(
                    self.pcm,
                    samples[sample_offset..].as_ptr().cast::<c_void>(),
                    remaining as c_ulong,
                )
            };

            if result > 0 {
                frame_offset += result as usize;
                continue;
            }
            if result == 0 {
                return Err("ALSA write returned zero frames".to_owned());
            }

            let error_code = result as c_int;
            let recovery = unsafe { snd_pcm_recover(self.pcm, error_code, 1) };
            if recovery < 0 {
                return Err(format!(
                    "ALSA write failed: {}; recovery failed: {}",
                    alsa_error_text(error_code),
                    alsa_error_text(recovery)
                ));
            }
            self.recoveries = self.recoveries.saturating_add(1);
            recovered = true;
        }
        Ok(recovered)
    }
}

impl Drop for AlsaPlayback {
    fn drop(&mut self) {
        if !self.pcm.is_null() {
            unsafe {
                snd_pcm_close(self.pcm);
            }
            self.pcm = ptr::null_mut();
        }
    }
}

fn configure_pcm(pcm: *mut c_void, config: &AlsaPlaybackConfig) -> Result<(usize, usize), String> {
    let mut raw_hw = ptr::null_mut();
    alsa_check(
        unsafe { snd_pcm_hw_params_malloc(&mut raw_hw) },
        "snd_pcm_hw_params_malloc",
    )?;
    let hw = HwParams(raw_hw);

    alsa_check(
        unsafe { snd_pcm_hw_params_any(pcm, hw.0) },
        "snd_pcm_hw_params_any",
    )?;
    alsa_check(
        unsafe { snd_pcm_hw_params_set_access(pcm, hw.0, SND_PCM_ACCESS_RW_INTERLEAVED) },
        "set RW_INTERLEAVED",
    )?;

    let format_name = CString::new("S32_LE").expect("static ALSA format name");
    let format = unsafe { snd_pcm_format_value(format_name.as_ptr()) };
    if format < 0 {
        return Err("ALSA does not recognize S32_LE".to_owned());
    }
    alsa_check(
        unsafe { snd_pcm_hw_params_set_format(pcm, hw.0, format) },
        "set S32_LE",
    )?;
    alsa_check(
        unsafe { snd_pcm_hw_params_set_channels(pcm, hw.0, config.channels as u32) },
        "set channel count",
    )?;
    alsa_check(
        unsafe { snd_pcm_hw_params_set_rate(pcm, hw.0, config.sample_rate, 0) },
        "set sample rate",
    )?;

    let mut requested_period = config.period_frames as c_ulong;
    let mut direction: c_int = 0;
    alsa_check(
        unsafe {
            snd_pcm_hw_params_set_period_size_near(pcm, hw.0, &mut requested_period, &mut direction)
        },
        "set ALSA period",
    )?;

    let mut requested_buffer = requested_period.saturating_mul(config.buffer_periods as c_ulong);
    alsa_check(
        unsafe { snd_pcm_hw_params_set_buffer_size_near(pcm, hw.0, &mut requested_buffer) },
        "set ALSA buffer",
    )?;
    alsa_check(
        unsafe { snd_pcm_hw_params(pcm, hw.0) },
        "apply ALSA hw params",
    )?;

    let mut actual_period: c_ulong = 0;
    direction = 0;
    alsa_check(
        unsafe { snd_pcm_hw_params_get_period_size(hw.0, &mut actual_period, &mut direction) },
        "read ALSA period",
    )?;
    let mut actual_buffer: c_ulong = 0;
    alsa_check(
        unsafe { snd_pcm_hw_params_get_buffer_size(hw.0, &mut actual_buffer) },
        "read ALSA buffer",
    )?;

    let period_frames = actual_period as usize;
    let buffer_frames = actual_buffer as usize;
    if period_frames == 0 || buffer_frames < period_frames.saturating_mul(2) {
        return Err(format!(
            "ALSA returned unsafe period/buffer geometry: period={period_frames} buffer={buffer_frames}"
        ));
    }

    configure_sw_params(pcm, period_frames, buffer_frames)?;
    alsa_check(unsafe { snd_pcm_prepare(pcm) }, "prepare ALSA PCM")?;

    Ok((period_frames, buffer_frames))
}

fn configure_sw_params(
    pcm: *mut c_void,
    period_frames: usize,
    buffer_frames: usize,
) -> Result<(), String> {
    let mut raw_sw = ptr::null_mut();
    alsa_check(
        unsafe { snd_pcm_sw_params_malloc(&mut raw_sw) },
        "snd_pcm_sw_params_malloc",
    )?;
    let sw = SwParams(raw_sw);

    alsa_check(
        unsafe { snd_pcm_sw_params_current(pcm, sw.0) },
        "snd_pcm_sw_params_current",
    )?;

    let start_threshold = buffer_frames.saturating_sub(period_frames) as c_ulong;
    alsa_check(
        unsafe { snd_pcm_sw_params_set_start_threshold(pcm, sw.0, start_threshold) },
        "set ALSA start threshold",
    )?;
    alsa_check(
        unsafe { snd_pcm_sw_params_set_avail_min(pcm, sw.0, period_frames as c_ulong) },
        "set ALSA avail_min",
    )?;
    alsa_check(
        unsafe { snd_pcm_sw_params_set_stop_threshold(pcm, sw.0, buffer_frames as c_ulong) },
        "set ALSA stop threshold",
    )?;
    alsa_check(
        unsafe { snd_pcm_sw_params(pcm, sw.0) },
        "apply ALSA sw params",
    )
}

struct HwParams(*mut c_void);
impl Drop for HwParams {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { snd_pcm_hw_params_free(self.0) };
        }
    }
}

struct SwParams(*mut c_void);
impl Drop for SwParams {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { snd_pcm_sw_params_free(self.0) };
        }
    }
}

fn alsa_check(code: c_int, operation: &str) -> Result<(), String> {
    if code < 0 {
        Err(format!("{operation}: {}", alsa_error_text(code)))
    } else {
        Ok(())
    }
}

fn alsa_error_text(code: c_int) -> String {
    let message = unsafe { snd_strerror(code) };
    if message.is_null() {
        return format!("ALSA error {code}");
    }
    unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned()
}

#[link(name = "asound")]
extern "C" {
    fn snd_pcm_open(
        pcm: *mut *mut c_void,
        name: *const c_char,
        stream: c_int,
        mode: c_int,
    ) -> c_int;
    fn snd_pcm_close(pcm: *mut c_void) -> c_int;
    fn snd_pcm_prepare(pcm: *mut c_void) -> c_int;
    fn snd_pcm_delay(pcm: *mut c_void, delay: *mut c_long) -> c_int;
    fn snd_pcm_writei(pcm: *mut c_void, buffer: *const c_void, frames: c_ulong) -> c_long;
    fn snd_pcm_recover(pcm: *mut c_void, error: c_int, silent: c_int) -> c_int;

    fn snd_pcm_hw_params_malloc(params: *mut *mut c_void) -> c_int;
    fn snd_pcm_hw_params_free(params: *mut c_void);
    fn snd_pcm_hw_params_any(pcm: *mut c_void, params: *mut c_void) -> c_int;
    fn snd_pcm_hw_params_set_access(pcm: *mut c_void, params: *mut c_void, access: c_int) -> c_int;
    fn snd_pcm_hw_params_set_format(pcm: *mut c_void, params: *mut c_void, format: c_int) -> c_int;
    fn snd_pcm_hw_params_set_channels(
        pcm: *mut c_void,
        params: *mut c_void,
        channels: u32,
    ) -> c_int;
    fn snd_pcm_hw_params_set_rate(
        pcm: *mut c_void,
        params: *mut c_void,
        rate: u32,
        direction: c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_set_period_size_near(
        pcm: *mut c_void,
        params: *mut c_void,
        frames: *mut c_ulong,
        direction: *mut c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_set_buffer_size_near(
        pcm: *mut c_void,
        params: *mut c_void,
        frames: *mut c_ulong,
    ) -> c_int;
    fn snd_pcm_hw_params_get_period_size(
        params: *mut c_void,
        frames: *mut c_ulong,
        direction: *mut c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_get_buffer_size(params: *mut c_void, frames: *mut c_ulong) -> c_int;
    fn snd_pcm_hw_params(pcm: *mut c_void, params: *mut c_void) -> c_int;

    fn snd_pcm_sw_params_malloc(params: *mut *mut c_void) -> c_int;
    fn snd_pcm_sw_params_free(params: *mut c_void);
    fn snd_pcm_sw_params_current(pcm: *mut c_void, params: *mut c_void) -> c_int;
    fn snd_pcm_sw_params_set_start_threshold(
        pcm: *mut c_void,
        params: *mut c_void,
        value: c_ulong,
    ) -> c_int;
    fn snd_pcm_sw_params_set_avail_min(
        pcm: *mut c_void,
        params: *mut c_void,
        value: c_ulong,
    ) -> c_int;
    fn snd_pcm_sw_params_set_stop_threshold(
        pcm: *mut c_void,
        params: *mut c_void,
        value: c_ulong,
    ) -> c_int;
    fn snd_pcm_sw_params(pcm: *mut c_void, params: *mut c_void) -> c_int;

    fn snd_pcm_format_value(name: *const c_char) -> c_int;
    fn snd_strerror(error: c_int) -> *const c_char;
}
