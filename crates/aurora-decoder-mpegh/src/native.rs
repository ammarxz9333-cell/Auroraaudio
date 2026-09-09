use std::ffi::c_void;

use aurora_core::AudioBlock;
use aurora_hoa_ir::{
    coefficient_count_for_order, HoaCoefficientBinding, HoaCoefficientConvention,
    HoaCoefficientFrame,
};
use thiserror::Error;

use crate::ffi::{
    self, IaMpeghdApiStruct, EXTERNAL_METADATA_BYTES, EXTERNAL_PCM_BYTES,
    MAX_HOA_COEFFICIENT_FLOATS, MAX_HOA_FRAME_LENGTH, MAX_HOA_ORDER, MEMTYPE_INPUT,
    MEMTYPE_OUTPUT,
};
use crate::MpeghRenderedPcm;

const NO_ERROR: i32 = 0x0000_0000;
const INIT_NEED_MORE_INPUT: i32 = 0x0000_1000;
const EXEC_NEED_MORE_INPUT: i32 = 0x0000_1800;
const PCM_WORD_SIZE_BITS: u32 = 16;
const MHAS_ENABLED: i32 = 1;
const DEFAULT_CICP_LAYOUT: i32 = 0;
const DEFAULT_EFFECT: i32 = 0;
const DEFAULT_PRESET: i8 = -1;
const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;
const MAX_RENDERED_CHANNELS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghExternalFrame {
    pub channel_metadata: Vec<u8>,
    pub object_metadata: Vec<u8>,
    pub hoa_metadata: Vec<u8>,
    pub prerender_pcm: Vec<u8>,
    pub pcm_bit_depth: i32,
    pub sample_rate: i32,
    pub oam_sample_offset: i32,
    pub hoa_sample_offset: i32,
    pub speaker_layout: MpeghSpeakerLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghSpeakerLayout {
    pub cicp_index: i32,
    pub layout_code: i32,
    pub speakers: Vec<MpeghSpeaker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpeghSpeaker {
    pub is_lfe: bool,
    pub azimuth_degrees: i16,
    pub elevation_degrees: i16,
}

/// Safe streaming owner for the pinned libmpegh decoder.
///
/// One successful execute can preserve three distinct products from the same
/// access unit: external-render scene material, post-spatial/pre-speaker HOA
/// ACN/N3D coefficients, and libmpegh's final speaker PCM reference.
pub struct NativeMpeghDecoder {
    api: Box<IaMpeghdApiStruct>,
    channel_metadata: Box<[u8]>,
    object_metadata: Box<[u8]>,
    hoa_metadata: Box<[u8]>,
    prerender_pcm: Box<[u8]>,
    hoa_coeff_buffer: Box<[f32]>,
    hoa_coeff_written: Box<u32>,
    hoa_coeff_order: Box<u32>,
    hoa_coeff_frame_length: Box<u32>,
    pending: Vec<u8>,
    pending_start: usize,
    created: bool,
    initialized: bool,
    last_rendered_pcm: Option<MpeghRenderedPcm>,
    last_hoa_coefficients: Option<HoaCoefficientFrame>,
}

impl NativeMpeghDecoder {
    pub fn new() -> Result<Self, MpeghNativeError> {
        let mut channel_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut object_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut hoa_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut prerender_pcm = vec![0_u8; EXTERNAL_PCM_BYTES].into_boxed_slice();
        let mut hoa_coeff_buffer =
            vec![0.0_f32; MAX_HOA_COEFFICIENT_FLOATS].into_boxed_slice();
        let mut hoa_coeff_written = Box::new(0_u32);
        let mut hoa_coeff_order = Box::new(0_u32);
        let mut hoa_coeff_frame_length = Box::new(0_u32);
        let mut api = Box::new(IaMpeghdApiStruct::default());

        api.input_config.ui_mhas_flag = MHAS_ENABLED;
        api.input_config.ui_pcm_wd_sz = PCM_WORD_SIZE_BITS;
        api.input_config.ui_cicp_layout_idx = DEFAULT_CICP_LAYOUT;
        api.input_config.ui_effect = DEFAULT_EFFECT;
        api.input_config.i_preset_id = DEFAULT_PRESET;
        api.input_config.extrn_rend_flag = 1;
        api.input_config.ptr_ext_ren_ch_data_buf = channel_metadata.as_mut_ptr();
        api.input_config.ptr_ext_ren_oam_data_buf = object_metadata.as_mut_ptr();
        api.input_config.ptr_ext_ren_hoa_data_buf = hoa_metadata.as_mut_ptr();
        api.input_config.ptr_ext_ren_pcm_buf = prerender_pcm.as_mut_ptr();
        api.output_config.malloc_mpegh = Some(malloc_mpegh);
        api.output_config.free_mpegh = Some(free_mpegh);

        let create_code = unsafe {
            ffi::ia_mpegh_dec_create(
                (&mut api.input_config as *mut _) as *mut c_void,
                (&mut api.output_config as *mut _) as *mut c_void,
            )
        };
        if create_code != NO_ERROR {
            return Err(MpeghNativeError::CreateFailed { code: create_code });
        }

        if api.output_config.pv_ia_process_api_obj.is_null() {
            delete_after_failed_create(&mut api);
            return Err(MpeghNativeError::InvalidLibraryState(
                "create returned a null process object",
            ));
        }
        let input_table = &api.output_config.mem_info_table[MEMTYPE_INPUT];
        if input_table.mem_ptr.is_null()
            || input_table.ui_size == 0
            || api.output_config.ui_inp_buf_size == 0
        {
            delete_after_failed_create(&mut api);
            return Err(MpeghNativeError::InvalidLibraryState(
                "create did not provide a usable compressed-input buffer",
            ));
        }

        let observer_code = unsafe {
            ffi::aurora_mpegh_set_hoa_coeff_observer(
                api.output_config.pv_ia_process_api_obj,
                hoa_coeff_buffer.as_mut_ptr(),
                u32::try_from(hoa_coeff_buffer.len()).map_err(|_| {
                    MpeghNativeError::InvalidLibraryState("HOA observer capacity overflow")
                })?,
                &mut *hoa_coeff_written,
                &mut *hoa_coeff_order,
                &mut *hoa_coeff_frame_length,
            )
        };
        if observer_code != NO_ERROR {
            delete_after_failed_create(&mut api);
            return Err(MpeghNativeError::HoaObserverRegistrationFailed {
                code: observer_code,
            });
        }

        Ok(Self {
            api,
            channel_metadata,
            object_metadata,
            hoa_metadata,
            prerender_pcm,
            hoa_coeff_buffer,
            hoa_coeff_written,
            hoa_coeff_order,
            hoa_coeff_frame_length,
            pending: Vec::new(),
            pending_start: 0,
            created: true,
            initialized: false,
            last_rendered_pcm: None,
            last_hoa_coefficients: None,
        })
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending.len().saturating_sub(self.pending_start)
    }

    pub fn take_rendered_pcm(&mut self) -> Option<MpeghRenderedPcm> {
        self.last_rendered_pcm.take()
    }

    /// Take the post-spatial, pre-speaker HOA coefficients produced by the
    /// most recent successful access unit. The frame is ACN/N3D and is never
    /// synthesized from external-render transport lanes.
    pub fn take_hoa_coefficients(&mut self) -> Option<HoaCoefficientFrame> {
        self.last_hoa_coefficients.take()
    }

    pub fn push(&mut self, input: &[u8]) -> Result<Option<MpeghExternalFrame>, MpeghNativeError> {
        self.append_input(input)?;
        if !self.initialized && !self.drive_initialization()? {
            return Ok(None);
        }
        self.execute_one()
    }

    fn append_input(&mut self, input: &[u8]) -> Result<(), MpeghNativeError> {
        if input.is_empty() {
            return Ok(());
        }
        self.compact_pending_if_useful();
        if self.pending_bytes().saturating_add(input.len()) > MAX_PENDING_BYTES {
            return Err(MpeghNativeError::PendingInputLimitExceeded {
                limit: MAX_PENDING_BYTES,
            });
        }
        self.pending.extend_from_slice(input);
        Ok(())
    }

    fn drive_initialization(&mut self) -> Result<bool, MpeghNativeError> {
        while !self.initialized {
            if self.pending_bytes() == 0 {
                return Ok(false);
            }
            let provided = self.fill_c_input()?;
            self.api.output_config.i_bytes_consumed = 0;
            let code = unsafe {
                ffi::ia_mpegh_dec_init(
                    self.api.output_config.pv_ia_process_api_obj,
                    (&mut self.api.input_config as *mut _) as *mut c_void,
                    (&mut self.api.output_config as *mut _) as *mut c_void,
                )
            };
            let consumed = self.consume_reported_bytes(provided)?;
            match code {
                NO_ERROR | INIT_NEED_MORE_INPUT => {}
                other => return Err(MpeghNativeError::InitFailed { code: other }),
            }
            if self.api.output_config.ui_init_done != 0 {
                self.initialized = true;
                return Ok(true);
            }
            if consumed == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn execute_one(&mut self) -> Result<Option<MpeghExternalFrame>, MpeghNativeError> {
        self.last_rendered_pcm = None;
        self.last_hoa_coefficients = None;
        self.clear_hoa_observer_state();
        if self.pending_bytes() == 0 {
            return Ok(None);
        }
        let provided = self.fill_c_input()?;
        self.api.output_config.i_bytes_consumed = 0;
        clear_external_lengths(&mut self.api);
        let code = unsafe {
            ffi::ia_mpegh_dec_execute(
                self.api.output_config.pv_ia_process_api_obj,
                (&mut self.api.input_config as *mut _) as *mut c_void,
                (&mut self.api.output_config as *mut _) as *mut c_void,
            )
        };
        self.consume_reported_bytes(provided)?;

        match code {
            NO_ERROR => {
                self.last_rendered_pcm = self.copy_rendered_pcm()?;
                self.last_hoa_coefficients = self.copy_hoa_coefficients()?;
                self.copy_external_frame()
            }
            EXEC_NEED_MORE_INPUT => Ok(None),
            other => Err(MpeghNativeError::ExecuteFailed { code: other }),
        }
    }

    fn clear_hoa_observer_state(&mut self) {
        *self.hoa_coeff_written = 0;
        *self.hoa_coeff_order = 0;
        *self.hoa_coeff_frame_length = 0;
    }

    fn copy_hoa_coefficients(&self) -> Result<Option<HoaCoefficientFrame>, MpeghNativeError> {
        hoa_frame_from_observer(
            &self.hoa_coeff_buffer,
            *self.hoa_coeff_written,
            *self.hoa_coeff_order,
            *self.hoa_coeff_frame_length,
        )
    }

    fn fill_c_input(&mut self) -> Result<usize, MpeghNativeError> {
        let api_capacity = usize::try_from(self.api.output_config.ui_inp_buf_size).map_err(|_| {
            MpeghNativeError::InvalidLibraryState("compressed-input API capacity overflow")
        })?;
        let input_table = &self.api.output_config.mem_info_table[MEMTYPE_INPUT];
        let table_capacity = usize::try_from(input_table.ui_size).map_err(|_| {
            MpeghNativeError::InvalidLibraryState("compressed-input table capacity overflow")
        })?;
        if input_table.mem_ptr.is_null() || api_capacity == 0 || table_capacity == 0 {
            return Err(MpeghNativeError::InvalidLibraryState(
                "compressed-input buffer disappeared after create",
            ));
        }
        let available = self.pending_bytes();
        let provided = api_capacity.min(table_capacity).min(available);
        if provided == 0 {
            return Ok(0);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.pending.as_ptr().add(self.pending_start),
                input_table.mem_ptr.cast::<u8>(),
                provided,
            );
        }
        self.api.input_config.num_inp_bytes = i32::try_from(provided)
            .map_err(|_| MpeghNativeError::InputFragmentTooLarge { bytes: provided })?;
        Ok(provided)
    }

    fn consume_reported_bytes(&mut self, provided: usize) -> Result<usize, MpeghNativeError> {
        let reported = self.api.output_config.i_bytes_consumed;
        let consumed = usize::try_from(reported)
            .map_err(|_| MpeghNativeError::InvalidConsumption { provided, reported })?;
        if consumed > provided {
            return Err(MpeghNativeError::InvalidConsumption { provided, reported });
        }
        self.pending_start = self.pending_start.saturating_add(consumed);
        if self.pending_start == self.pending.len() {
            self.pending.clear();
            self.pending_start = 0;
        } else {
            self.compact_pending_if_useful();
        }
        Ok(consumed)
    }

    fn compact_pending_if_useful(&mut self) {
        if self.pending_start == 0 {
            return;
        }
        if self.pending_start >= 64 * 1024 || self.pending_start * 2 >= self.pending.len() {
            self.pending.copy_within(self.pending_start.., 0);
            let remaining = self.pending.len() - self.pending_start;
            self.pending.truncate(remaining);
            self.pending_start = 0;
        }
    }

    fn copy_external_frame(&self) -> Result<Option<MpeghExternalFrame>, MpeghNativeError> {
        let output = &self.api.output_config;
        let ch_len = validated_payload_len(
            "channel metadata",
            output.ch_data_present,
            output.ch_md_payload_length,
            self.channel_metadata.len(),
        )?;
        let oam_len = validated_payload_len(
            "object metadata",
            output.oam_data_present,
            output.oam_md_payload_length,
            self.object_metadata.len(),
        )?;
        let hoa_len = validated_payload_len(
            "HOA metadata",
            output.hoa_data_present,
            output.hoa_md_payload_length,
            self.hoa_metadata.len(),
        )?;
        let pcm_len = checked_len(
            "pre-render PCM",
            output.pcm_payload_length,
            self.prerender_pcm.len(),
        )?;
        if ch_len == 0 && oam_len == 0 && hoa_len == 0 && pcm_len == 0 {
            return Ok(None);
        }

        let speaker_count = usize::try_from(output.num_speakers)
            .map_err(|_| MpeghNativeError::InvalidSpeakerCount(output.num_speakers))?;
        if speaker_count > output.is_lfe.len() {
            return Err(MpeghNativeError::InvalidSpeakerCount(output.num_speakers));
        }
        let speakers = (0..speaker_count)
            .map(|index| MpeghSpeaker {
                is_lfe: output.is_lfe[index] != 0,
                azimuth_degrees: output.azimuth[index],
                elevation_degrees: output.elevation[index],
            })
            .collect();

        Ok(Some(MpeghExternalFrame {
            channel_metadata: self.channel_metadata[..ch_len].to_vec(),
            object_metadata: self.object_metadata[..oam_len].to_vec(),
            hoa_metadata: self.hoa_metadata[..hoa_len].to_vec(),
            prerender_pcm: self.prerender_pcm[..pcm_len].to_vec(),
            pcm_bit_depth: output.pcm_bit_depth,
            sample_rate: output.i_samp_freq,
            oam_sample_offset: output.oam_sample_offset,
            hoa_sample_offset: output.hoa_sample_offset,
            speaker_layout: MpeghSpeakerLayout {
                cicp_index: output.cicp_index,
                layout_code: output.spk_layout,
                speakers,
            },
        }))
    }

    fn copy_rendered_pcm(&self) -> Result<Option<MpeghRenderedPcm>, MpeghNativeError> {
        let output = &self.api.output_config;
        if output.num_out_bytes == 0 {
            return Ok(None);
        }
        let table = &output.mem_info_table[MEMTYPE_OUTPUT];
        let capacity = usize::try_from(table.ui_size).map_err(|_| {
            MpeghNativeError::InvalidLibraryState("rendered-output buffer capacity overflow")
        })?;
        let length = checked_len("rendered PCM", output.num_out_bytes, capacity)?;
        if length == 0 {
            return Ok(None);
        }
        if table.mem_ptr.is_null() {
            return Err(MpeghNativeError::InvalidLibraryState(
                "rendered-output buffer is null with non-zero output length",
            ));
        }

        let bit_depth = u8::try_from(output.i_pcm_wd_sz)
            .map_err(|_| MpeghNativeError::InvalidRenderedBitDepth(output.i_pcm_wd_sz))?;
        let bytes_per_sample = match bit_depth {
            16 => 2usize,
            24 => 3usize,
            32 => 4usize,
            _ => return Err(MpeghNativeError::InvalidRenderedBitDepth(output.i_pcm_wd_sz)),
        };
        let channel_count = usize::try_from(output.i_num_chan)
            .ok()
            .filter(|channels| (1..=MAX_RENDERED_CHANNELS).contains(channels))
            .ok_or(MpeghNativeError::InvalidRenderedChannelCount(output.i_num_chan))?;
        let sample_rate = u32::try_from(output.i_samp_freq)
            .ok()
            .filter(|rate| *rate > 0)
            .ok_or(MpeghNativeError::InvalidRenderedSampleRate(output.i_samp_freq))?;
        let bytes_per_frame = channel_count
            .checked_mul(bytes_per_sample)
            .ok_or(MpeghNativeError::RenderedGeometryOverflow)?;
        if length % bytes_per_frame != 0 {
            return Err(MpeghNativeError::MisalignedRenderedPcm {
                bytes: length,
                channels: channel_count,
                bit_depth,
            });
        }
        let frame_count = length / bytes_per_frame;
        if frame_count == 0 {
            return Ok(None);
        }
        let bytes = unsafe { core::slice::from_raw_parts(table.mem_ptr.cast::<u8>(), length) }
            .to_vec();
        let rendered = MpeghRenderedPcm {
            bytes,
            bit_depth,
            channel_count,
            frame_count,
            sample_rate,
        };
        rendered
            .validate()
            .map_err(|error| MpeghNativeError::InvalidRenderedPcm(error.to_string()))?;
        Ok(Some(rendered))
    }
}

impl Drop for NativeMpeghDecoder {
    fn drop(&mut self) {
        if self.created {
            unsafe {
                let _ = ffi::ia_mpegh_dec_delete(
                    (&mut self.api.output_config as *mut _) as *mut c_void,
                );
            }
            self.created = false;
        }
    }
}

fn hoa_frame_from_observer(
    buffer: &[f32],
    written_floats: u32,
    order_raw: u32,
    frame_length_raw: u32,
) -> Result<Option<HoaCoefficientFrame>, MpeghNativeError> {
    if written_floats == 0 {
        if order_raw != 0 || frame_length_raw != 0 {
            return Err(MpeghNativeError::InvalidHoaObserverGeometry {
                order: order_raw,
                frame_length: frame_length_raw,
                written_floats,
            });
        }
        return Ok(None);
    }
    if order_raw > MAX_HOA_ORDER {
        return Err(MpeghNativeError::InvalidHoaObserverGeometry {
            order: order_raw,
            frame_length: frame_length_raw,
            written_floats,
        });
    }
    let frame_length = usize::try_from(frame_length_raw).map_err(|_| {
        MpeghNativeError::InvalidHoaObserverGeometry {
            order: order_raw,
            frame_length: frame_length_raw,
            written_floats,
        }
    })?;
    if frame_length == 0 || frame_length > MAX_HOA_FRAME_LENGTH {
        return Err(MpeghNativeError::InvalidHoaObserverGeometry {
            order: order_raw,
            frame_length: frame_length_raw,
            written_floats,
        });
    }
    let order = u16::try_from(order_raw).map_err(|_| MpeghNativeError::InvalidHoaObserverGeometry {
        order: order_raw,
        frame_length: frame_length_raw,
        written_floats,
    })?;
    let coefficient_count = coefficient_count_for_order(order)
        .map_err(|error| MpeghNativeError::InvalidHoaCoefficientFrame(error.to_string()))?;
    let expected = coefficient_count
        .checked_mul(frame_length)
        .ok_or(MpeghNativeError::HoaObserverGeometryOverflow)?;
    let written = usize::try_from(written_floats)
        .map_err(|_| MpeghNativeError::HoaObserverGeometryOverflow)?;
    if written != expected || written > buffer.len() {
        return Err(MpeghNativeError::InvalidHoaObserverGeometry {
            order: order_raw,
            frame_length: frame_length_raw,
            written_floats,
        });
    }

    let channels = (0..coefficient_count)
        .map(|index| {
            let start = index * frame_length;
            buffer[start..start + frame_length].to_vec()
        })
        .collect::<Vec<_>>();
    let coefficients = (0..coefficient_count)
        .map(|index| HoaCoefficientBinding {
            pcm_channel_index: index,
            coefficient_index: index,
        })
        .collect();
    let frame = HoaCoefficientFrame {
        audio: AudioBlock {
            channels,
            frame_count: frame_length,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        order,
        convention: HoaCoefficientConvention::AcnN3d,
        coefficients,
    };
    frame
        .validate()
        .map_err(|error| MpeghNativeError::InvalidHoaCoefficientFrame(error.to_string()))?;
    Ok(Some(frame))
}

fn delete_after_failed_create(api: &mut IaMpeghdApiStruct) {
    unsafe {
        let _ = ffi::ia_mpegh_dec_delete((&mut api.output_config as *mut _) as *mut c_void);
    }
}

fn clear_external_lengths(api: &mut IaMpeghdApiStruct) {
    let output = &mut api.output_config;
    output.ch_md_payload_length = 0;
    output.oam_md_payload_length = 0;
    output.hoa_md_payload_length = 0;
    output.pcm_payload_length = 0;
    output.num_out_bytes = 0;
}

fn checked_len(
    plane: &'static str,
    reported: i32,
    capacity: usize,
) -> Result<usize, MpeghNativeError> {
    let len = usize::try_from(reported).map_err(|_| MpeghNativeError::InvalidPayloadLength {
        plane,
        reported,
        capacity,
    })?;
    if len > capacity {
        return Err(MpeghNativeError::InvalidPayloadLength {
            plane,
            reported,
            capacity,
        });
    }
    Ok(len)
}

fn validated_payload_len(
    plane: &'static str,
    present: i32,
    reported: i32,
    capacity: usize,
) -> Result<usize, MpeghNativeError> {
    if present == 0 {
        Ok(0)
    } else {
        checked_len(plane, reported, capacity)
    }
}

unsafe extern "C" fn malloc_mpegh(size: u32, alignment: u32) -> *mut c_void {
    let Some(bytes) = size.checked_add(alignment) else {
        return core::ptr::null_mut();
    };
    unsafe { libc::malloc(bytes as usize) }
}

unsafe extern "C" fn free_mpegh(pointer: *mut c_void) {
    unsafe { libc::free(pointer) }
}

#[derive(Debug, Error)]
pub enum MpeghNativeError {
    #[error("libmpegh create failed with error 0x{code:08x}")]
    CreateFailed { code: i32 },
    #[error("Aurora HOA observer registration failed with error {code}")]
    HoaObserverRegistrationFailed { code: i32 },
    #[error("libmpegh initialization failed with error 0x{code:08x}")]
    InitFailed { code: i32 },
    #[error("libmpegh execution failed with error 0x{code:08x}")]
    ExecuteFailed { code: i32 },
    #[error("libmpegh returned invalid internal state: {0}")]
    InvalidLibraryState(&'static str),
    #[error("libmpegh reported consuming {reported} bytes after Aurora provided {provided}")]
    InvalidConsumption { provided: usize, reported: i32 },
    #[error("compressed MPEG-H fragment is too large for the C API: {bytes} bytes")]
    InputFragmentTooLarge { bytes: usize },
    #[error("MPEG-H pending compressed input exceeded {limit} bytes")]
    PendingInputLimitExceeded { limit: usize },
    #[error("libmpegh reported invalid {plane} length {reported} for buffer capacity {capacity}")]
    InvalidPayloadLength {
        plane: &'static str,
        reported: i32,
        capacity: usize,
    },
    #[error("libmpegh reported invalid speaker count {0}")]
    InvalidSpeakerCount(i32),
    #[error("libmpegh reported unsupported rendered PCM bit depth {0}")]
    InvalidRenderedBitDepth(i32),
    #[error("libmpegh reported invalid rendered channel count {0}")]
    InvalidRenderedChannelCount(i32),
    #[error("libmpegh reported invalid rendered sample rate {0}")]
    InvalidRenderedSampleRate(i32),
    #[error("rendered PCM geometry arithmetic overflow")]
    RenderedGeometryOverflow,
    #[error("rendered PCM byte length {bytes} is not aligned to {channels} channels at {bit_depth} bits")]
    MisalignedRenderedPcm {
        bytes: usize,
        channels: usize,
        bit_depth: u8,
    },
    #[error("rendered PCM failed Aurora validation: {0}")]
    InvalidRenderedPcm(String),
    #[error("HOA observer geometry overflow")]
    HoaObserverGeometryOverflow,
    #[error("invalid HOA observer geometry: order={order}, frame_length={frame_length}, written_floats={written_floats}")]
    InvalidHoaObserverGeometry {
        order: u32,
        frame_length: u32,
        written_floats: u32,
    },
    #[error("HOA coefficient frame failed Aurora validation: {0}")]
    InvalidHoaCoefficientFrame(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_buffer_sizes_match_upstream_testbench_contract() {
        assert_eq!(EXTERNAL_METADATA_BYTES, 768);
        assert_eq!(EXTERNAL_PCM_BYTES, 131_072);
        assert_eq!(MAX_HOA_COEFFICIENT_FLOATS, 50_176);
    }

    #[test]
    fn payload_length_validation_fails_closed() {
        assert_eq!(checked_len("x", 5, 8).unwrap(), 5);
        assert!(checked_len("x", -1, 8).is_err());
        assert!(checked_len("x", 9, 8).is_err());
    }

    #[test]
    fn pending_cursor_compacts_without_losing_bytes() {
        let mut pending = b"abcdef".to_vec();
        let start = 3;
        pending.copy_within(start.., 0);
        pending.truncate(pending.len() - start);
        assert_eq!(&pending, b"def");
    }

    #[test]
    fn observer_snapshot_becomes_complete_acn_n3d_frame() {
        let frame_length = 4usize;
        let mut buffer = vec![0.0_f32; MAX_HOA_COEFFICIENT_FLOATS];
        for coefficient in 0..4usize {
            for sample in 0..frame_length {
                buffer[coefficient * frame_length + sample] =
                    (coefficient * 10 + sample) as f32;
            }
        }
        let frame = hoa_frame_from_observer(&buffer, 16, 1, 4)
            .unwrap()
            .unwrap();
        assert_eq!(frame.order, 1);
        assert_eq!(frame.convention, HoaCoefficientConvention::AcnN3d);
        assert_eq!(frame.audio.channels.len(), 4);
        assert_eq!(frame.audio.channels[2], vec![20.0, 21.0, 22.0, 23.0]);
        frame.validate().unwrap();
    }

    #[test]
    fn observer_snapshot_rejects_partial_coefficient_payload() {
        let buffer = vec![0.0_f32; MAX_HOA_COEFFICIENT_FLOATS];
        assert!(matches!(
            hoa_frame_from_observer(&buffer, 15, 1, 4),
            Err(MpeghNativeError::InvalidHoaObserverGeometry { .. })
        ));
    }
}