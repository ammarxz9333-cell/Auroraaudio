#![allow(non_camel_case_types)]
#![allow(dead_code)]

use libc::c_void;

pub type IaErrorCode = i32;

pub const MEMTYPE_PERSIST: usize = 0;
pub const MEMTYPE_SCRATCH: usize = 1;
pub const MEMTYPE_INPUT: usize = 2;
pub const MEMTYPE_OUTPUT: usize = 3;
pub const EXTERNAL_METADATA_BYTES: usize = 768;
pub const EXTERNAL_PCM_BYTES: usize = 1024 * 32 * 4;

pub type MallocFn = unsafe extern "C" fn(u32, u32) -> *mut c_void;
pub type FreeFn = unsafe extern "C" fn(*mut c_void);
pub type DeMallocFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IaMemInfoTable {
    pub ui_size: u32,
    pub ui_alignment: u32,
    pub ui_type: u32,
    pub mem_ptr: *mut c_void,
}

impl Default for IaMemInfoTable {
    fn default() -> Self {
        Self {
            ui_size: 0,
            ui_alignment: 0,
            ui_type: 0,
            mem_ptr: core::ptr::null_mut(),
        }
    }
}

#[repr(C)]
pub struct IaInputConfig {
    pub ui_pcm_wd_sz: u32,
    pub ui_effect: i32,
    pub ui_target_loudness: [i32; 2],
    pub ui_mhas_flag: i32,
    pub ui_raw_flag: i32,
    pub ui_cicp_layout_idx: i32,
    pub i_preset_id: i8,
    pub num_inp_bytes: i32,
    pub lsi_info_flag: i32,
    pub ei_info_flag: i32,
    pub sd_info_flag: i32,
    pub lsi_info_size: i32,
    pub ei_info_size: i32,
    pub sd_info_size: i32,
    pub extrn_rend_flag: i32,
    pub ptr_ei_buf: *mut u8,
    pub ptr_ls_buf: *mut u8,
    pub ptr_sd_buf: *mut u8,
    pub ptr_ext_ren_ch_data_buf: *mut u8,
    pub ptr_ext_ren_oam_data_buf: *mut u8,
    pub ptr_ext_ren_hoa_data_buf: *mut u8,
    pub ptr_ext_ren_pcm_buf: *mut u8,
    pub ptr_maei_buf: *mut u8,
    pub ptr_maeg_buf: *mut u8,
    pub ptr_maes_buf: *mut u8,
    pub ptr_maep_buf: *mut u8,
    pub maei_flag: i32,
    pub maeg_flag: i32,
    pub maes_flag: i32,
    pub maep_flag: i32,
    pub maei_len: i32,
    pub maeg_len: i32,
    pub maes_len: i32,
    pub maep_len: i32,
    pub binaural_flag: i32,
    pub binaural_data_len: i32,
    pub ptr_brir_buf: *mut u8,
    pub enable_resamp: i32,
    pub out_samp_freq: u32,
}

impl Default for IaInputConfig {
    fn default() -> Self {
        // The C testbench zero-initializes the full structure before applying
        // documented defaults. Zero is therefore the ABI-safe baseline.
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
pub struct IaOutputConfig {
    pub i_samp_freq: i32,
    pub i_num_chan: i32,
    pub i_pcm_wd_sz: i32,
    pub i_drc_effect: i32,
    pub i_target_loudness: i32,
    pub i_loud_norm: i32,
    pub num_preroll: i32,
    pub ui_init_done: u32,
    pub num_out_bytes: i32,
    pub i_bytes_consumed: i32,
    pub ui_inp_buf_size: u32,
    pub malloc_count: u32,
    pub ui_rem: u32,
    pub ui_proc_mem_tabs_size: u32,
    pub pv_ia_process_api_obj: *mut c_void,
    pub arr_alloc_memory: [*mut c_void; 100],
    pub p_lib_name: *mut i8,
    pub p_version_num: *mut i8,
    pub malloc_mpegh: Option<MallocFn>,
    pub free_mpegh: Option<FreeFn>,
    pub de_malloc_xc: Option<DeMallocFn>,
    pub mem_info_table: [IaMemInfoTable; 4],
    pub oam_data_present: i32,
    pub oam_md_payload_length: i32,
    pub hoa_data_present: i32,
    pub hoa_md_payload_length: i32,
    pub ch_data_present: i32,
    pub ch_md_payload_length: i32,
    pub pcm_payload_length: i32,
    pub pcm_bit_depth: i32,
    pub hoa_sample_offset: i32,
    pub oam_sample_offset: i32,
    pub num_speakers: i32,
    pub spk_layout: i32,
    pub cicp_index: i32,
    pub is_binaural_rendering: i32,
    pub is_lfe: [i32; 24],
    pub azimuth: [i16; 24],
    pub elevation: [i16; 24],
}

impl Default for IaOutputConfig {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct IaMpeghdApiStruct {
    pub input_config: IaInputConfig,
    pub output_config: IaOutputConfig,
}

unsafe extern "C" {
    pub fn ia_mpegh_dec_get_lib_id_strings(output: *mut c_void) -> IaErrorCode;
    pub fn ia_mpegh_dec_create(input: *mut c_void, output: *mut c_void) -> IaErrorCode;
    pub fn ia_mpegh_dec_init(
        module: *mut c_void,
        input: *mut c_void,
        output: *mut c_void,
    ) -> IaErrorCode;
    pub fn ia_mpegh_dec_execute(
        module: *mut c_void,
        input: *mut c_void,
        output: *mut c_void,
    ) -> IaErrorCode;
    pub fn ia_mpegh_dec_delete(output: *mut c_void) -> IaErrorCode;
}
