#[path = "../iec61937.rs"]
mod iec61937;
#[path = "../orender_host.rs"]
mod orender_host;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-joc-e2e-probe is Linux-only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = run() {
        eprintln!("aurora-joc-e2e-probe: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    use iec61937::{s32_samples_to_iec_words, BurstParser, CodecFilter};
    use orender_host::{OrenderHost, OrenderHostConfig};
    use std::env;
    use std::fs;

    let mut args = env::args().skip(1);
    let library_path = args.next().ok_or("missing liborender.so path")?;
    let bridge_path = args.next().ok_or("missing Harletty bridge path")?;
    let speaker_layout_path = args.next().ok_or("missing Aurora layout path")?;
    let fixture_path = args.next().ok_or("missing raw E-AC-3 JOC fixture path")?;
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_owned());
    }

    let stream = fs::read(&fixture_path)
        .map_err(|error| format!("failed to read {fixture_path}: {error}"))?;
    let access_units = split_eac3_access_units(&stream)?;
    if access_units.len() < 10 {
        return Err(format!(
            "fixture is too short for a streaming JOC test: {} access units",
            access_units.len()
        ));
    }

    let mut host = OrenderHost::open(&OrenderHostConfig {
        library_path,
        bridge_path,
        speaker_layout_path,
        config_yaml_path: None,
    })?;
    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let mut iec_words = Vec::new();

    let mut parsed_bursts = 0usize;
    let mut render_callbacks = 0usize;
    let mut rendered_frames = 0usize;
    let mut peak = 0.0_f32;
    let mut channel_energy = [0.0_f64; 12];
    let mut saw_objects = false;
    let mut max_objects = 0u32;

    for access_unit in &access_units {
        let carrier = wrap_eac3_as_s32_iec61937(access_unit)?;

        // Deliberately split the simulated ALSA capture at an odd period so
        // IEC preambles/payloads cross capture boundaries. This exercises the
        // same stateful reassembly used by the live R2 daemon.
        for capture_chunk in carrier.chunks(257) {
            s32_samples_to_iec_words(capture_chunk, &mut iec_words);
            let mut callback_error: Option<String> = None;
            parser.push_each(&iec_words, |data_type, payload| {
                if callback_error.is_some() {
                    return;
                }
                if data_type != 0x15 {
                    callback_error =
                        Some(format!("unexpected IEC61937 data type 0x{data_type:02x}"));
                    return;
                }
                if payload != *access_unit {
                    callback_error = Some("IEC61937 payload was not bit-exact".to_owned());
                    return;
                }

                parsed_bursts += 1;
                let result = host.process_raw(payload, |samples, frames| {
                    if samples.len() != frames.saturating_mul(12) {
                        callback_error =
                            Some("renderer output is not 12-channel aligned".to_owned());
                        return;
                    }
                    render_callbacks += 1;
                    rendered_frames = rendered_frames.saturating_add(frames);
                    for frame in samples.chunks_exact(12) {
                        for (channel, sample) in frame.iter().copied().enumerate() {
                            if !sample.is_finite() {
                                callback_error =
                                    Some(format!("non-finite PCM on channel {channel}"));
                                return;
                            }
                            peak = peak.max(sample.abs());
                            channel_energy[channel] += f64::from(sample) * f64::from(sample);
                        }
                    }
                });
                if let Err(error) = result {
                    callback_error = Some(error);
                    return;
                }
                saw_objects |= host.has_objects();
                max_objects = max_objects.max(host.object_count());
            });
            if let Some(error) = callback_error {
                return Err(error);
            }
        }
    }

    if parsed_bursts != access_units.len() {
        return Err(format!(
            "IEC61937 parser emitted {parsed_bursts} bursts for {} access units",
            access_units.len()
        ));
    }
    if render_callbacks == 0 || rendered_frames < 48_000 {
        return Err(format!(
            "renderer produced too little PCM: callbacks={render_callbacks} frames={rendered_frames}"
        ));
    }
    if !(peak.is_finite() && peak > 1.0e-6 && peak <= 8.0) {
        return Err(format!("implausible rendered PCM peak: {peak}"));
    }
    if !saw_objects || max_objects == 0 {
        return Err(format!(
            "JOC object telemetry never became active: saw_objects={saw_objects} max_objects={max_objects}"
        ));
    }

    let height_energy: f64 = channel_energy[8..12].iter().sum();
    let bed_energy: f64 = channel_energy[..8].iter().sum();
    if height_energy <= 1.0e-12 {
        return Err(format!(
            "7.1.4 height outputs stayed silent; bed_energy={bed_energy:.9e} height_energy={height_energy:.9e}"
        ));
    }

    println!(
        "real JOC E2E: PASS access_units={} bursts={} callbacks={} frames={} peak={:.6} max_objects={} bed_energy={:.6e} height_energy={:.6e} renderer={}",
        access_units.len(),
        parsed_bursts,
        render_callbacks,
        rendered_frames,
        peak,
        max_objects,
        bed_energy,
        height_energy,
        host.build_id()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn split_eac3_access_units(stream: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < stream.len() {
        if stream.len() - offset < 4 {
            return Err(format!("truncated E-AC-3 header at byte {offset}"));
        }
        if stream[offset..offset + 2] != [0x0B, 0x77] {
            return Err(format!("missing E-AC-3 sync word at byte {offset}"));
        }
        let header = u16::from_be_bytes([stream[offset + 2], stream[offset + 3]]);
        let frame_size = (usize::from(header & 0x07FF) + 1) * 2;
        if frame_size < 8 || offset.saturating_add(frame_size) > stream.len() {
            return Err(format!(
                "invalid E-AC-3 frame size {frame_size} at byte {offset}"
            ));
        }
        out.push(&stream[offset..offset + frame_size]);
        offset += frame_size;
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn wrap_eac3_as_s32_iec61937(access_unit: &[u8]) -> Result<Vec<i32>, String> {
    if access_unit.is_empty() || access_unit.len() % 2 != 0 {
        return Err("E-AC-3 access unit must be non-empty and word-aligned".to_owned());
    }
    let payload_len = u16::try_from(access_unit.len())
        .map_err(|_| format!("E-AC-3 access unit too large: {} bytes", access_unit.len()))?;

    let mut burst = Vec::with_capacity(8 + access_unit.len());
    burst.extend_from_slice(&[0x72, 0xF8]); // Pa, little-endian 16-bit word.
    burst.extend_from_slice(&[0x1F, 0x4E]); // Pb.
    burst.extend_from_slice(&0x0015_u16.to_le_bytes()); // Pc = E-AC-3/DD+.
    burst.extend_from_slice(&payload_len.to_le_bytes()); // Pd is bytes for type 0x15.
    for word in access_unit.chunks_exact(2) {
        // IEC61937 payload words are byte-swapped relative to the native E-AC-3 AU.
        burst.extend_from_slice(&[word[1], word[0]]);
    }

    let mut carrier = Vec::with_capacity(burst.len() / 2 + 32);
    // A little zero spacing exercises preamble search without pretending to
    // model the physical IEC repetition period in this software-only fixture.
    carrier.extend(std::iter::repeat(0_i32).take(17));
    for word in burst.chunks_exact(2) {
        let high_word = u32::from(u16::from_le_bytes([word[0], word[1]])) << 16;
        carrier.push(high_word as i32);
    }
    carrier.extend(std::iter::repeat(0_i32).take(13));
    Ok(carrier)
}
