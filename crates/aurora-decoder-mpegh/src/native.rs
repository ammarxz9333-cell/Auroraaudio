use std::ffi::c_void;

use thiserror::Error;

use crate::ffi::{
    self, IaMpeghdApiStruct, EXTERNAL_METADATA_BYTES, EXTERNAL_PCM_BYTES, MEMTYPE_INPUT,
};

const NO_ERROR: i32 = 0x0000_0000;
const INIT_NEED_MORE_INPUT: i32 = 0x0000_1000;
const EXEC_NEED_MORE_INPUT: i32 = 0x0000_1800;
const PCM_WORD_SIZE_BITS: u32 = 16;
const MHAS_ENABLED: i32 = 1;
const DEFAULT_CICP_LAYOUT: i32 = 0;
const DEFAULT_EFFECT: i32 = 0;
const DEFAULT_PRESET: i8 = -1;
const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;

/// One native MPEG-H external-render result copied out of libmpegh-owned or
/// caller-owned C buffers before the next decoder call can overwrite them.
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

/// Streaming wrapper around libmpegh's external-render API.
///
/// All unsafe C interaction stays inside this module. Compressed bytes remain
/// owned by Aurora until libmpegh reports them consumed, so fragmented network
/// or IEC61937 delivery cannot lose an access-unit tail.
pub struct NativeMpeghDecoder {
    api: Box<IaMpeghdApiStruct>,
    channel_metadata: Box<[u8]>,
    object_metadata: Box<[u8]>,
    hoa_metadata: Box<[u8]>,
    prerender_pcm: Box<[u8]>,
    pending: Vec<u8>,
    created: bool,
    initialized: bool,
}

impl NativeMpeghDecoder {
    pub fn new() -> Result<Self, MpeghNativeError> {
        let mut channel_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut object_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut hoa_metadata = vec![0_u8; EXTERNAL_METADATA_BYTES].into_boxed_slice();
        let mut prerender_pcm = vec![0_u8; EXTERNAL_PCM_BYTES].into_boxed_slice();
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
            // create succeeded, so delete before returning the invariant error.
            unsafe {
                let _ = ffi::ia_mpegh_dec_delete(
                    (&mut api.output_config as *mut _) as *mut c_void,
                );
            }
            return Err(MpeghNativeError::InvalidLibraryState(
                "create returned a null process object",
            ));
        }
        let input_table = api.output_config.mem_info_table.get(MEMTYPE_INPUT).ok_or(
            MpeghNativeError::InvalidLibraryState("input memory table is unavailable"),
        )?;
        if input_table.mem_ptr.is_null() || api.output_config.ui_inp_buf_size == 0 {
            unsafe {
                let _ = ffi::ia_mpegh_dec_delete(
                    (&mut api.output_config as *mut _) as *mut c_void,
                );
            }
            return Err(MpeghNativeError::InvalidLibraryState(
                "create did not provide a usable compressed-input buffer",
            ));
        }

        Ok(Self {
            api,
            channel_metadata,
            object_metadata,
            hoa_metadata,
            prerender_pcm,
            pending: Vec::new(),
            created: true,
            initialized: false,
        })
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    /// Append arbitrary compressed fragments and return at most one decoded
    /// external-render frame. Call again with an empty slice to drain buffered
    /// compressed data.
    pub fn push(&mut self, input: &[u8]) -> Result<Option<MpeghExternalFrame>, MpeghNativeError> {
        if self.pending.len().saturating_add(input.len()) > MAX_PENDING_BYTES {
            return Err(MpeghNativeError::PendingInputLimitExceeded {
                limit: MAX_PENDING_BYTES,
            });
        }
        self.pending.extend_from_slice(input);

        if !self.initialized && !self.drive_initialization()? {
            return Ok(None);
        }
        self.execute_one()
    }

    fn drive_initialization(&mut self) -> Result<bool, MpeghNativeError> {
        while !self.initialized {
            if self.pending.is_empty() {
                return Ok(false);
            }
            let provided = self.fill_c_input()?;
            let module = self.api.output_config.pv_ia_process_api_obj;
            self.api.output_config.i_bytes_consumed = 0;
            let code = unsafe {
                ffi::ia_mpegh_dec_init(
                    module,
                    (&mut self.api.input_config as *mut _) as *mut c_void,
                    (&mut self.api.output_config as *mut _) as *mut c_void,
                )
            };
            self.consume_reported_bytes(provided)?;

            if self.api.output_config.ui_init_done != 0 {
                self.initialized = true;
                break;
            }
            match code {
                NO_ERROR | INIT_NEED_MORE_INPUT => {
                    if self.api.output_config.i_bytes_consumed == 0 {
                        return Ok(false);
                    }
                }
                other => return Err(MpeghNativeError::InitFailed { code: other }),
            }
        }
        Ok(self.initialized)
    }

    fn execute_one(&mut self) -> Result<Option<MpeghExternalFrame>, MpeghNativeError> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        let provided = self.fill_c_input()?;
        let module = self.api.output_config.pv_ia_process_api_obj;
        self.api.output_config.i_bytes_consumed = 0;
        clear_external_lengths(&mut self.api);
        let code = unsafe {
            ffi::ia_mpegh_dec_execute(
                module,
                (&mut self.api.input_config as *mut _) as *mut c_void,
                (&mut self.api.output_config as *mut _) as *mut c_void,
            )
        };
        self.consume_reported_bytes(provided)?;

        match code {
            NO_ERROR => {}
            EXEC_NEED_MORE_INPUT => return Ok(None),
            other => return Err(MpeghNativeError::ExecuteFailed { code: other }),
        }
        self.copy_external_frame()
    }

    fn fill_c_input(&mut self) -> Result<usize, MpeghNativeError> {
        let capacity = usize::try_from(self.api.output_config.ui_inp_buf_size).map_err(|_| {
            MpeghNativeError::InvalidLibraryState("compressed-input capacity overflow")
        })?;
        let input_table = &self.api.output_config.mem_info_table[MEMTYPE_INPUT];
        if input_table.mem_ptr.is_null() || capacity == 0 {
            return Err(MpeghNativeError::InvalidLibraryState(
                "compressed-input buffer disappeared after initialization",
            ));
        }
        let provided = capacity.min(self.pending.len());
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.pending.as_ptr(),
                input_table.mem_ptr.cast::<u8>(),
                provided,
            );
        }
        self.api.input_config.num_inp_bytes = i32::try_from(provided)
            .map_err(|_| MpeghNativeError::InputFragmentTooLarge { bytes: provided })?;
        Ok(provided)
    }

    fn consume_reported_bytes(&mut self, provided: usize) -> Result<(), MpeghNativeError> {
        let consumed = usize::try_from(self.api.output_config.i_bytes_consumed).map_err(|_| {
            MpeghNativeError::InvalidConsumption {
                provided,
                reported: self.api.output_config.i_bytes_consumed,
            }
        })?;
        if consumed > provided {
            return Err(MpeghNativeError::InvalidConsumption {
                provided,
                reported: self.api.output_config.i_bytes_consumed,
            });
        }
        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        Ok(())
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
        let pcm_len = checked_len("pre-render PCM", output.pcm_payload_length, self.prerender_pcm.len())?;

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

fn clear_external_lengths(api: &mut IaMpeghdApiStruct) {
    let output = &mut api.output_config;
    output.ch_md_payload_length = 0;
    output.oam_md_payload_length = 0;
    output.hoa_md_payload_length = 0;
    output.pcm_payload_length = 0;
}

fn checked_len(
    plane: &'static str,
    reported: i32,
    capacity: usize,
) -> Result<usize, MpeghNativeError> {
    let len = usize::try_from(reported)
        .map_err(|_| MpeghNativeError::InvalidPayloadLength { plane, reported, capacity })?;
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
        return Ok(0);
    }
    checked_len(plane, reported, capacity)
}

unsafe extern "C" fn malloc_mpegh(size: u32, alignment: u32) -> *mut c_void {
    let bytes = size.saturating_add(alignment) as usize;
    unsafe { libc::malloc(bytes) }
}

unsafe extern "C" fn free_mpegh(pointer: *mut c_void) {
    unsafe { libc::free(pointer) }
}

#[derive(Debug, Error)]
pub enum MpeghNativeError {
    #[error("libmpegh create failed with error 0x{code:08x}")]
    CreateFailed { code: i32 },
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_buffer_sizes_match_upstream_testbench_contract() {
        assert_eq!(EXTERNAL_METADATA_BYTES, 768);
        assert_eq!(EXTERNAL_PCM_BYTES, 131_072);
    }

    #[test]
    fn payload_length_validation_fails_closed() {
        assert_eq!(checked_len("x", 5, 8).unwrap(), 5);
        assert!(checked_len("x", -1, 8).is_err());
        assert!(checked_len("x", 9, 8).is_err());
    }
}
