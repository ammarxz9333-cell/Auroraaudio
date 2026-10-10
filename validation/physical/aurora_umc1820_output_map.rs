use std::{
    env,
    io::{self, BufReader, BufWriter, Read, Write},
};

const AURORA_CHANNELS: usize = 12;
const UMC_PLAYBACK_CHANNELS: usize = 20;
const S32_BYTES: usize = 4;
const S24_3_BYTES: usize = 3;

// Aurora canonical 7.1.4 -> UMC1820 USB playback channel (1-based):
// FL  -> 3   (UMC line out 3)
// FR  -> 4
// FC  -> 5
// LFE -> 6
// SL  -> 7
// SR  -> 8
// SBL -> 9
// SBR -> 10
// TFL -> 13  (ADAT 1 -> ADA8200 out 1)
// TFR -> 14  (ADAT 2 -> ADA8200 out 2)
// TRL -> 15  (ADAT 3 -> ADA8200 out 3)
// TRR -> 16  (ADAT 4 -> ADA8200 out 4)
//
// USB playback 1-2 (UMC main), 11-12 (S/PDIF), and 17-20
// (remaining ADAT lanes) are intentionally muted.
const AURORA_TO_UMC_ZERO_BASED: [usize; AURORA_CHANNELS] =
    [2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15];

fn s32_to_s24_3(sample: i32) -> [u8; 3] {
    // Aurora S32_LE uses the full signed i32 range. The UMC1820 Linux USB
    // playback endpoint is S24_3LE, so retain the most significant 24 bits.
    let value = sample >> 8;
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

fn map_frame(input: &[u8], output: &mut [u8]) -> Result<(), &'static str> {
    if input.len() != AURORA_CHANNELS * S32_BYTES
        || output.len() != UMC_PLAYBACK_CHANNELS * S24_3_BYTES
    {
        return Err("invalid frame shape");
    }
    output.fill(0);
    for (aurora_channel, &umc_channel) in AURORA_TO_UMC_ZERO_BASED.iter().enumerate() {
        let source = aurora_channel * S32_BYTES;
        let sample = i32::from_le_bytes([
            input[source],
            input[source + 1],
            input[source + 2],
            input[source + 3],
        ]);
        let converted = s32_to_s24_3(sample);
        let target = umc_channel * S24_3_BYTES;
        output[target..target + S24_3_BYTES].copy_from_slice(&converted);
    }
    Ok(())
}

fn run_self_test() -> Result<(), String> {
    let mut input = [0_u8; AURORA_CHANNELS * S32_BYTES];
    for channel in 0..AURORA_CHANNELS {
        let sample = ((channel as i32 + 1) * 0x0101_0100).to_le_bytes();
        input[channel * S32_BYTES..(channel + 1) * S32_BYTES].copy_from_slice(&sample);
    }
    let mut output = [0xAA_u8; UMC_PLAYBACK_CHANNELS * S24_3_BYTES];
    map_frame(&input, &mut output).map_err(str::to_owned)?;

    let mut seen = [false; UMC_PLAYBACK_CHANNELS];
    for (aurora_channel, &umc_channel) in AURORA_TO_UMC_ZERO_BASED.iter().enumerate() {
        seen[umc_channel] = true;
        let expected_sample =
            ((aurora_channel as i32 + 1) * 0x0101_0100) >> 8;
        let expected_bytes = expected_sample.to_le_bytes();
        let target = umc_channel * S24_3_BYTES;
        if output[target..target + S24_3_BYTES] != expected_bytes[..3] {
            return Err(format!(
                "channel mapping mismatch: Aurora {} -> UMC {}",
                aurora_channel + 1,
                umc_channel + 1
            ));
        }
    }
    for channel in 0..UMC_PLAYBACK_CHANNELS {
        if !seen[channel] {
            let start = channel * S24_3_BYTES;
            if output[start..start + S24_3_BYTES] != [0, 0, 0] {
                return Err(format!("unused UMC channel {} is not muted", channel + 1));
            }
        }
    }

    eprintln!(
        "AURORA-UMC1820-MAP-SELFTEST-PASS input=s32le/12ch output=s24_3le/20ch map=3-10,13-16"
    );
    Ok(())
}

fn run_stream() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());

    let input_frame_bytes = AURORA_CHANNELS * S32_BYTES;
    let output_frame_bytes = UMC_PLAYBACK_CHANNELS * S24_3_BYTES;
    let mut input_frame = vec![0_u8; input_frame_bytes];
    let mut output_frame = vec![0_u8; output_frame_bytes];
    let mut frames = 0_u64;

    loop {
        let mut filled = 0usize;
        while filled < input_frame_bytes {
            let read = input.read(&mut input_frame[filled..])?;
            if read == 0 {
                if filled == 0 {
                    output.flush()?;
                    eprintln!(
                        "AURORA-UMC1820-MAP-PASS frames={} input=s32le/12ch output=s24_3le/20ch",
                        frames
                    );
                    return Ok(());
                }
                return Err(format!(
                    "partial Aurora frame at EOF: {filled}/{input_frame_bytes} bytes"
                )
                .into());
            }
            filled += read;
        }

        map_frame(&input_frame, &mut output_frame)?;
        output.write_all(&output_frame)?;
        frames = frames.saturating_add(1);
    }
}

fn main() {
    let mut self_test = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--self-test" => self_test = true,
            "-h" | "--help" => {
                eprintln!(
                    "usage: aurora-umc1820-map [--self-test]\n\
                     stdin:  Aurora S32_LE 12ch 48k canonical 7.1.4\n\
                     stdout: UMC1820 S24_3LE 20ch USB playback layout"
                );
                return;
            }
            _ => {
                eprintln!("AURORA-UMC1820-MAP-FAIL: unknown argument: {arg}");
                std::process::exit(2);
            }
        }
    }

    let result = if self_test {
        run_self_test().map_err(|error| error.into())
    } else {
        run_stream()
    };
    if let Err(error) = result {
        eprintln!("AURORA-UMC1820-MAP-FAIL: {error}");
        std::process::exit(1);
    }
}
