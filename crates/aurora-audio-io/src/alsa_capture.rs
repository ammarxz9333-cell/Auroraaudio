//! Minimal direct ALSA capture wrapper for the Aurora R2 appliance.
//!
//! The capture contract is intentionally exact: S32_LE, 2 channels, 192 kHz.
//! No ALSA plug/conversion layer is used, because the upper 16 bits carry the
//! IEC 61937 word stream and any format conversion would corrupt it.

#![cfg(target_os = "linux")]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
use std::ptr;

const SND_PCM_STREAM_CAPTURE: c_int = 1;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

#[derive(Debug, Clone)]
pub struct AlsaCaptureConfig {
    pub device: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub period_frames: usize,
    pub buffer_periods: usize,
}

#[derive(Debug)]
pub struct AlsaCapture {
    pcm: *mut c_void,
    channels: usize,
    period_frames: usize,
    buffer_frames: usize,
    recoveries: u64,
}

impl AlsaCapture {
    pub fn open(config: &AlsaCaptureConfig) -> Result<Self, String> {
        if config.channels == 0 || config.period_frames == 0 || config.buffer_periods < 2 {
            return Err("invalid ALSA capture configuration".to_owned());
        }
        let device = CString::new(config.device.as_str())
            .map_err(|_| "ALSA capture device name contains NUL".to_owned())?;
        let mut pcm = ptr::null_mut();
        alsa_check(
            // SAFETY: `pcm` is writable pointer storage and device is a live C string.
            unsafe { snd_pcm_open(&mut pcm, device.as_ptr(), SND_PCM_STREAM_CAPTURE, 0) },
            "snd_pcm_open(capture)",
        )?;

        let setup = configure_capture(pcm, config);
        let (period_frames, buffer_frames) = match setup {
            Ok(values) => values,
            Err(error) => {
                // SAFETY: handle came from successful snd_pcm_open.
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

    /// Fill `samples` with complete interleaved capture frames.
    pub fn read_interleaved(&mut self, samples: &mut [i32]) -> Result<(), String> {
        if samples.len() % self.channels != 0 {
            return Err("capture buffer is not frame-aligned".to_owned());
        }
        let total_frames = samples.len() / self.channels;
        let mut frame_offset = 0usize;
        while frame_offset < total_frames {
            let sample_offset = frame_offset * self.channels;
            let remaining = total_frames - frame_offset;
            let result =
                // SAFETY: destination slice is valid and frame-aligned for this blocking call.
                unsafe {
                    snd_pcm_readi(
                        self.pcm,
                        samples[sample_offset..].as_mut_ptr().cast::<c_void>(),
                        remaining as c_ulong,
                    )
                };
            if result > 0 {
                frame_offset += result as usize;
                continue;
            }
            if result == 0 {
                return Err("ALSA capture returned zero frames".to_owned());
            }
            let error_code = result as c_int;
            let recovered =
                // SAFETY: error code was returned by snd_pcm_readi for this live PCM.
                unsafe { snd_pcm_recover(self.pcm, error_code, 1) };
            if recovered < 0 {
                return Err(format!(
                    "ALSA capture failed: {}; recovery failed: {}",
                    alsa_error_text(error_code),
                    alsa_error_text(recovered)
                ));
            }
            self.recoveries = self.recoveries.saturating_add(1);
        }
        Ok(())
    }
}

impl Drop for AlsaCapture {
    fn drop(&mut self) {
        if !self.pcm.is_null() {
            // SAFETY: unique ownership of the PCM handle.
            unsafe { snd_pcm_close(self.pcm) };
            self.pcm = ptr::null_mut();
        }
    }
}

fn configure_capture(
    pcm: *mut c_void,
    config: &AlsaCaptureConfig,
) -> Result<(usize, usize), String> {
    let mut raw_hw = ptr::null_mut();
    alsa_check(
        // SAFETY: writable out-pointer.
        unsafe { snd_pcm_hw_params_malloc(&mut raw_hw) },
        "snd_pcm_hw_params_malloc(capture)",
    )?;
    let hw = HwParams(raw_hw);
    alsa_check(
        // SAFETY: live handles.
        unsafe { snd_pcm_hw_params_any(pcm, hw.0) },
        "snd_pcm_hw_params_any(capture)",
    )?;
    alsa_check(
        // SAFETY: valid params object.
        unsafe { snd_pcm_hw_params_set_access(pcm, hw.0, SND_PCM_ACCESS_RW_INTERLEAVED) },
        "set capture RW_INTERLEAVED",
    )?;

    let format_name = CString::new("S32_LE").expect("static ALSA format name");
    let format =
        // SAFETY: valid NUL-terminated C string.
        unsafe { snd_pcm_format_value(format_name.as_ptr()) };
    if format < 0 {
        return Err("ALSA does not recognize S32_LE".to_owned());
    }
    alsa_check(
        // SAFETY: valid live handles.
        unsafe { snd_pcm_hw_params_set_format(pcm, hw.0, format) },
        "set capture S32_LE",
    )?;
    alsa_check(
        // SAFETY: valid live handles.
        unsafe { snd_pcm_hw_params_set_channels(pcm, hw.0, config.channels as u32) },
        "set capture channels",
    )?;
    alsa_check(
        // SAFETY: exact rate requested; dir=0 forbids near-rate substitution.
        unsafe { snd_pcm_hw_params_set_rate(pcm, hw.0, config.sample_rate, 0) },
        "set capture sample rate",
    )?;

    let mut period = config.period_frames as c_ulong;
    let mut direction: c_int = 0;
    alsa_check(
        // SAFETY: writable stack out-parameters.
        unsafe { snd_pcm_hw_params_set_period_size_near(pcm, hw.0, &mut period, &mut direction) },
        "set capture period",
    )?;
    let mut buffer = period.saturating_mul(config.buffer_periods as c_ulong);
    alsa_check(
        // SAFETY: writable stack out-parameter.
        unsafe { snd_pcm_hw_params_set_buffer_size_near(pcm, hw.0, &mut buffer) },
        "set capture buffer",
    )?;
    alsa_check(
        // SAFETY: apply complete parameter set.
        unsafe { snd_pcm_hw_params(pcm, hw.0) },
        "apply capture hw params",
    )?;

    let mut actual_period: c_ulong = 0;
    direction = 0;
    alsa_check(
        // SAFETY: reads applied values into stack storage.
        unsafe { snd_pcm_hw_params_get_period_size(hw.0, &mut actual_period, &mut direction) },
        "read capture period",
    )?;
    let mut actual_buffer: c_ulong = 0;
    alsa_check(
        // SAFETY: reads applied value into stack storage.
        unsafe { snd_pcm_hw_params_get_buffer_size(hw.0, &mut actual_buffer) },
        "read capture buffer",
    )?;
    if actual_period == 0 || actual_buffer < actual_period.saturating_mul(2) {
        return Err(format!(
            "unsafe capture geometry period={} buffer={}",
            actual_period, actual_buffer
        ));
    }

    configure_capture_sw(pcm, actual_period as usize)?;
    alsa_check(
        // SAFETY: configured capture PCM.
        unsafe { snd_pcm_prepare(pcm) },
        "prepare capture PCM",
    )?;
    Ok((actual_period as usize, actual_buffer as usize))
}

fn configure_capture_sw(pcm: *mut c_void, period_frames: usize) -> Result<(), String> {
    let mut raw_sw = ptr::null_mut();
    alsa_check(
        // SAFETY: writable out-pointer.
        unsafe { snd_pcm_sw_params_malloc(&mut raw_sw) },
        "snd_pcm_sw_params_malloc(capture)",
    )?;
    let sw = SwParams(raw_sw);
    alsa_check(
        // SAFETY: live PCM and software-parameter object.
        unsafe { snd_pcm_sw_params_current(pcm, sw.0) },
        "snd_pcm_sw_params_current(capture)",
    )?;
    alsa_check(
        // SAFETY: period-sized wakeups reduce scheduler churn without conversion.
        unsafe { snd_pcm_sw_params_set_avail_min(pcm, sw.0, period_frames as c_ulong) },
        "set capture avail_min",
    )?;
    alsa_check(
        // SAFETY: apply initialized software params.
        unsafe { snd_pcm_sw_params(pcm, sw.0) },
        "apply capture sw params",
    )
}

struct HwParams(*mut c_void);
impl Drop for HwParams {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer came from ALSA malloc function.
            unsafe { snd_pcm_hw_params_free(self.0) };
        }
    }
}

struct SwParams(*mut c_void);
impl Drop for SwParams {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer came from ALSA malloc function.
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
    let message =
        // SAFETY: ALSA returns static C string or null.
        unsafe { snd_strerror(code) };
    if message.is_null() {
        return format!("ALSA error {code}");
    }
    // SAFETY: non-null return is NUL-terminated.
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
    fn snd_pcm_readi(pcm: *mut c_void, buffer: *mut c_void, frames: c_ulong) -> c_long;
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
    fn snd_pcm_sw_params_set_avail_min(
        pcm: *mut c_void,
        params: *mut c_void,
        value: c_ulong,
    ) -> c_int;
    fn snd_pcm_sw_params(pcm: *mut c_void, params: *mut c_void) -> c_int;
    fn snd_pcm_format_value(name: *const c_char) -> c_int;
    fn snd_strerror(error: c_int) -> *const c_char;
}
