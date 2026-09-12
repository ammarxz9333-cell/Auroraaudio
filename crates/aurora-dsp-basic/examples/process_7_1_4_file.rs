use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};

use anyhow::{bail, Context, Result};
use aurora_core::StandardLayout;
use aurora_dsp_basic::output::{
    ChannelCalibration, OutputDspConfig, SpeakerCalibration, SpeakerPostProcessor, CHANNELS,
    SAMPLE_RATE,
};

const SAMPLE_BYTES: usize = 4;
const CHUNK_FRAMES: usize = 4096;

fn flat_calibration() -> SpeakerCalibration {
    SpeakerCalibration {
        schema_version: 1,
        sample_rate: SAMPLE_RATE,
        channels: StandardLayout::SevenOneFour
            .canonical_roles()
            .iter()
            .map(|role| ChannelCalibration {
                role: role.clone(),
                trim_db: 0.0,
                delay_frames: 0,
                invert_polarity: false,
                peq: Vec::new(),
            })
            .collect(),
    }
}

fn decode_f32_le(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % SAMPLE_BYTES != 0 {
        bail!("input is not aligned to 32-bit floating-point samples");
    }
    Ok(bytes
        .chunks_exact(SAMPLE_BYTES)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
        .collect())
}

fn encode_f32_le(samples: &[f32], output: &mut BufWriter<File>) -> Result<()> {
    for sample in samples {
        output.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let input_path = args.next().context("usage: process_7_1_4_file <input.f32> <output.f32> [lipsync_frames]")?;
    let output_path = args.next().context("usage: process_7_1_4_file <input.f32> <output.f32> [lipsync_frames]")?;
    let lipsync_frames = match args.next() {
        Some(value) => value.parse::<usize>().context("invalid lipsync_frames")?,
        None => 0,
    };
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let mut processor = SpeakerPostProcessor::new(OutputDspConfig {
        lipsync_frames,
        ..OutputDspConfig::default()
    })?;
    // Exercise the exact transactional calibration preparation path even for
    // the flat baseline used by the deterministic simulator.
    processor.configure_calibration(&flat_calibration())?;

    let mut input = BufReader::new(File::open(&input_path).with_context(|| format!("open {input_path}"))?);
    let mut output = BufWriter::new(File::create(&output_path).with_context(|| format!("create {output_path}"))?);
    let frame_bytes = CHANNELS * SAMPLE_BYTES;
    let mut raw = vec![0_u8; CHUNK_FRAMES * frame_bytes];
    let mut frames = 0_u64;
    let mut peak = 0.0_f32;
    let mut non_finite = 0_u64;

    loop {
        let mut used = 0usize;
        while used < raw.len() {
            let count = input.read(&mut raw[used..])?;
            if count == 0 {
                break;
            }
            used += count;
        }
        if used == 0 {
            break;
        }
        if used % frame_bytes != 0 {
            bail!("input byte count is not aligned to twelve-channel f32 frames");
        }
        let mut samples = decode_f32_le(&raw[..used])?;
        processor.process_block(&mut samples)?;
        for sample in &samples {
            if sample.is_finite() {
                peak = peak.max(sample.abs());
            } else {
                non_finite += 1;
            }
        }
        if non_finite != 0 {
            bail!("output DSP emitted non-finite samples");
        }
        encode_f32_le(&samples, &mut output)?;
        frames += (samples.len() / CHANNELS) as u64;
        if used < raw.len() {
            break;
        }
    }
    output.flush()?;
    if frames == 0 {
        bail!("input contains no complete audio frames");
    }

    println!(
        "AURORA-OUTPUT-DSP-FILE-PASS frames={frames} channels={CHANNELS} sample_rate={SAMPLE_RATE} peak={peak:.9} lipsync_frames={lipsync_frames}"
    );
    Ok(())
}
