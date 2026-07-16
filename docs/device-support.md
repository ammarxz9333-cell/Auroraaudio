# Device Support

Milestone 0F supports local desktop audio enumeration and output through the CPAL backend.

## Windows

The selected backend is `cpal`, a permissively licensed Rust audio library. On Windows, CPAL uses the available Windows host support, including WASAPI where practical. Aurora only exposes `aurora-realtime-audio-api` types to the rest of the project.

## Device IDs

Aurora reports an owned descriptor containing backend, direction, device name,
host identifier when available, default channels, and default sample rate. Its
selector uses backend, direction, and host identifier; when CPAL supplies no
Windows endpoint GUID it falls back to normalized device name. Default format is
not part of the selector. Renaming a device can still change a CPAL selector, and
duplicate names may remain ambiguous. Run-local numeric indices remain accepted
as a temporary CLI convenience.

## Known Limits

- Only f32 callback streams are opened by the first backend.
- Channel count validation depends on the information reported by the operating system and driver.
- Hardware integration tests are ignored unless explicitly enabled by environment variables or manual CLI runs.
- No HDMI/eARC, wireless speaker transport, network audio, or mobile device control is included.
## Live Duplex Requirements

Live duplex requires one CPAL f32 input and one CPAL f32 output supporting the
requested channel count and rates. Stereo-only hardware is reported as stereo;
Aurora does not infer multichannel capability. Differing input/output rates can
be requested explicitly and are bridged by ASRC when both devices accept them.

CPAL does not provide a stable Windows endpoint GUID in the current boundary.
Selectors include backend, direction, normalized name or host identifier when
available, channel count, and default sample-rate hints. Exact identity selectors
are preferred; ambiguous fuzzy matches are rejected. Numeric indices are valid
only for the current enumeration.
