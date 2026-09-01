//! Streaming IEC 61937 extractor for Aurora's eARC input.

#[path = "../iec61937.rs"]
mod iec61937;

use std::io::{self, Read, Write};

use iec61937::{BurstParser, CodecFilter, S32HighWordAdapter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Width {
    S16,
    S32,
}

fn main() -> io::Result<()> {
    let (width, filter) = parse_args();
    let mut parser = BurstParser::new(filter);
    let mut s32 = S32HighWordAdapter::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut raw = [0_u8; 16 * 1024];
    let mut burst_count = 0_u64;
    let mut payload_bytes = 0_u64;
    let mut last_type = None;

    loop {
        let read = input.read(&mut raw)?;
        if read == 0 {
            break;
        }
        let words = match width {
            Width::S16 => raw[..read].to_vec(),
            Width::S32 => s32.push(&raw[..read]),
        };
        for burst in parser.push(&words) {
            last_type = Some(burst.data_type);
            burst_count += 1;
            payload_bytes += burst.payload.len() as u64;
            output.write_all(&burst.payload)?;
        }
    }
    output.flush()?;
    eprintln!(
        "aurora-iec61937-extract: bursts={burst_count} payload_bytes={payload_bytes} last_type={last_type:?}"
    );
    Ok(())
}

fn parse_args() -> (Width, CodecFilter) {
    let mut width = Width::S32;
    let mut filter = CodecFilter::Eac3;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--width" => {
                let Some(value) = args.next() else {
                    usage_and_exit();
                };
                width = match value.as_str() {
                    "s16" => Width::S16,
                    "s32" => Width::S32,
                    _ => usage_and_exit(),
                };
            }
            "--codec" => {
                let Some(value) = args.next() else {
                    usage_and_exit();
                };
                filter = match value.as_str() {
                    "ac3" => CodecFilter::Ac3,
                    "eac3" => CodecFilter::Eac3,
                    "dts" => CodecFilter::Dts,
                    "all" => CodecFilter::All,
                    _ => usage_and_exit(),
                };
            }
            "-h" | "--help" => {
                println!(
                    "Usage: aurora-iec61937-extract [--width s32|s16] [--codec eac3|ac3|dts|all]\n\
                     Defaults: --width s32 --codec eac3. Reads capture bytes on stdin and writes native codec payloads on stdout."
                );
                std::process::exit(0);
            }
            _ => usage_and_exit(),
        }
    }

    (width, filter)
}

fn usage_and_exit() -> ! {
    eprintln!("Usage: aurora-iec61937-extract [--width s32|s16] [--codec eac3|ac3|dts|all]");
    std::process::exit(2);
}
