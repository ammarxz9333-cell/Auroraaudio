#[path = "../orender_host.rs"]
mod orender_host;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-height-bed-probe is Linux-only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = run() {
        eprintln!("aurora-height-bed-probe: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    use orender_host::{OrenderHost, OrenderHostConfig};
    use std::env;

    let mut args = env::args().skip(1);
    let library_path = args.next().ok_or("missing liborender.so path")?;
    let bridge_path = args.next().ok_or("missing reference bridge path")?;
    let speaker_layout_path = args.next().ok_or("missing Aurora layout path")?;
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_owned());
    }

    let wav = make_height_only_wav();
    let mut host = OrenderHost::open(&OrenderHostConfig {
        library_path,
        bridge_path,
        speaker_layout_path,
        config_yaml_path: None,
    })?;

    let mut rendered_frames = 0usize;
    let mut peak = 0.0_f32;
    let mut channel_energy = [0.0_f64; 12];
    let mut callback_error: Option<String> = None;

    host.process_raw(&wav, |samples, frames| {
        if samples.len() != frames.saturating_mul(12) {
            callback_error = Some("renderer output is not 12-channel aligned".to_owned());
            return;
        }
        rendered_frames = rendered_frames.saturating_add(frames);
        for frame in samples.chunks_exact(12) {
            for (channel, sample) in frame.iter().copied().enumerate() {
                if !sample.is_finite() {
                    callback_error = Some(format!("non-finite PCM on channel {channel}"));
                    return;
                }
                peak = peak.max(sample.abs());
                channel_energy[channel] += f64::from(sample) * f64::from(sample);
            }
        }
    })?;
    if let Some(error) = callback_error {
        return Err(error);
    }

    if rendered_frames < 2_048 {
        return Err(format!(
            "reference bridge produced too little PCM: frames={rendered_frames}"
        ));
    }
    if !(peak.is_finite() && peak > 1.0e-6 && peak <= 2.0) {
        return Err(format!("implausible rendered PCM peak: {peak}"));
    }

    let mut height_energy = 0.0_f64;
    for channel in 8..12 {
        let energy = channel_energy[channel];
        if energy <= 1.0e-10 {
            return Err(format!(
                "height output channel {channel} stayed silent; energies={channel_energy:?}"
            ));
        }
        height_energy += energy;
    }
    let lower_energy: f64 = channel_energy[..8].iter().sum();
    if lower_energy > height_energy * 1.0e-5 + 1.0e-10 {
        return Err(format!(
            "height-only reference bed leaked into lower outputs: lower_energy={lower_energy:.9e} height_energy={height_energy:.9e}"
        ));
    }

    println!(
        "7.1.4 height bed: PASS frames={} peak={:.6} TFL={:.6e} TFR={:.6e} TRL={:.6e} TRR={:.6e} lower_energy={:.6e} renderer={}",
        rendered_frames,
        peak,
        channel_energy[8],
        channel_energy[9],
        channel_energy[10],
        channel_energy[11],
        lower_energy,
        host.build_id()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn make_height_only_wav() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: u16 = 12;
    const BITS_PER_SAMPLE: u16 = 16;
    const FRAMES: usize = 4_096;
    const HEIGHT_AMPLITUDES: [i16; 4] = [5_000, 7_000, 9_000, 11_000];
    const HEIGHT_PERIODS: [usize; 4] = [11, 17, 23, 29];

    let bytes_per_sample = usize::from(BITS_PER_SAMPLE / 8);
    let data_len = FRAMES * usize::from(CHANNELS) * bytes_per_sample;
    let riff_len = 36usize + data_len;
    let byte_rate = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8);
    let block_align = CHANNELS * (BITS_PER_SAMPLE / 8);

    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(riff_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());

    for frame in 0..FRAMES {
        for channel in 0..usize::from(CHANNELS) {
            let sample = if (8..12).contains(&channel) {
                let height = channel - 8;
                let sign = if (frame / HEIGHT_PERIODS[height]) % 2 == 0 {
                    1i16
                } else {
                    -1i16
                };
                HEIGHT_AMPLITUDES[height] * sign
            } else {
                0i16
            };
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }
    wav
}
