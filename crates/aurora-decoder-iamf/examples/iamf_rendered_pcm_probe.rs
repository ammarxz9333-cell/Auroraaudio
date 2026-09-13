#[cfg(feature = "libiamf-process")]
use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

#[cfg(feature = "libiamf-process")]
use aurora_core::{AudioFormat, SampleType};
#[cfg(feature = "libiamf-process")]
use aurora_decoder_api::{Decoder, DecoderOutputSemantics};
#[cfg(feature = "libiamf-process")]
use aurora_decoder_iamf::IamfRenderedPcmReferenceDecoder;

#[cfg(feature = "libiamf-process")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let executable = PathBuf::from(
        args.next()
            .ok_or("usage: iamf_rendered_pcm_probe IAMFDEC INPUT_IAMF OUTPUT_F32LE")?,
    );
    let input_path = PathBuf::from(
        args.next()
            .ok_or("usage: iamf_rendered_pcm_probe IAMFDEC INPUT_IAMF OUTPUT_F32LE")?,
    );
    let output_path = PathBuf::from(
        args.next()
            .ok_or("usage: iamf_rendered_pcm_probe IAMFDEC INPUT_IAMF OUTPUT_F32LE")?,
    );
    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    let input = fs::read(&input_path)?;
    let mut decoder = IamfRenderedPcmReferenceDecoder::new(executable);
    if decoder.info().output_semantics != DecoderOutputSemantics::ChannelPcm {
        return Err("IAMF rendered reference must report ChannelPcm semantics".into());
    }
    decoder.configure(AudioFormat {
        sample_rate: 48_000,
        channel_count: 2,
        sample_type: SampleType::F32,
        block_size: 256,
    })?;

    let decoded = decoder
        .decode_chunk(&input)?
        .ok_or("IAMF rendered reference emitted no frame")?;
    if !decoded.objects.is_empty() {
        return Err("rendered ChannelPcm reference must not fabricate objects".into());
    }
    decoded.audio.validate()?;
    if decoded.audio.channels.len() != 2 || decoded.audio.frame_count == 0 {
        return Err("rendered ChannelPcm reference did not emit non-empty stereo PCM".into());
    }

    let mut writer = BufWriter::new(File::create(&output_path)?);
    let mut energy = 0.0f64;
    for frame_index in 0..decoded.audio.frame_count {
        for channel in &decoded.audio.channels {
            let sample = channel[frame_index];
            if !sample.is_finite() {
                return Err("rendered ChannelPcm reference emitted non-finite PCM".into());
            }
            energy += f64::from(sample) * f64::from(sample);
            writer.write_all(&sample.to_le_bytes())?;
        }
    }
    writer.flush()?;
    if energy <= 0.0 {
        return Err("rendered ChannelPcm reference emitted silent PCM".into());
    }

    println!(
        "AURORA-IAMF-RENDERED-PCM-PASS frames={} channels=2 sample_rate=48000 objects=0 output={}",
        decoded.audio.frame_count,
        output_path.display()
    );
    Ok(())
}

#[cfg(not(feature = "libiamf-process"))]
fn main() {
    eprintln!("iamf_rendered_pcm_probe requires --features libiamf-process");
    std::process::exit(2);
}
