# CamillaDSP channel-order boundary

Aurora's canonical logical order and WAVE_FORMAT_EXTENSIBLE physical file order are not identical for 7.1 and 7.1.4.

Aurora 7.1.4 canonical order is:

`FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR`

WAVE_FORMAT_EXTENSIBLE serializes standard speaker roles in ascending speaker-mask-bit order:

`FL, FR, FC, LFE, SBL, SBR, SL, SR, TFL, TFR, TRL, TRR`

Therefore a DSP chain expressed as Aurora logical channel 4 (`SL`) must target physical WAV channel 6, while Aurora logical channel 6 (`SBL`) must target physical WAV channel 4. The equivalent mapping for 7.1.4 is:

`[0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11]`

Front center/dialogue remains index 2.

## Adapter contract

The original numeric CamillaDSP adapter APIs remain backward-compatible and interpret `ChannelDspConfig.channel` as a physical file-order channel index.

For Aurora logical channel indices, callers must use the role-aware APIs:

- `wav_file_channel_index_for_layout`;
- `remap_config_for_wav_layout`;
- `generate_camilladsp_yaml_for_layout`;
- `process_offline_wav_for_layout`;
- `CamillaDspOfflineAdapter::process_wav_for_layout`.

These APIs require a standard `StandardLayout`. They reject custom layouts, layout/channel-count mismatches, out-of-range channels, and duplicate logical channel entries instead of guessing.

## Why the WAV writer is not changed

Aurora's role-aware WAV writer already emits samples in the ordering required by the WAVE speaker mask. Changing the writer to preserve Aurora's logical array order would create a malformed semantic relationship between sample positions and the WAVE channel mask. The mapping therefore belongs at the DSP adapter boundary.

## Evidence boundary

Unit tests prove the deterministic logical-to-file permutation and preservation of distinct per-channel DSP parameters, including front-center stability and side/back-surround exchange. The Phase 10 CamillaDSP differential separately validates supported exported DSP semantics against the exact pinned external CamillaDSP binary.

This is a software channel-order contract. It is not physical DAC channel-map evidence; electrical output mapping still requires the separate physical validation lane.
