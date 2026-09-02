#[path = "../orender_host.rs"]
mod orender_host;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-orender-probe is Linux-only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = run() {
        eprintln!("aurora-orender-probe: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    use orender_host::{OrenderHost, OrenderHostConfig};
    use std::env;

    let mut args = env::args().skip(1);
    let library_path = args.next().ok_or("missing liborender.so path")?;
    let bridge_path = args.next().ok_or("missing bridge placeholder path")?;
    let speaker_layout_path = args.next().ok_or("missing layout placeholder path")?;
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_owned());
    }

    let mut host = OrenderHost::open(&OrenderHostConfig {
        library_path,
        bridge_path,
        speaker_layout_path,
        config_yaml_path: None,
    })?;

    if host.build_id() != "aurora-ci-liborender-stub" {
        return Err(format!("unexpected build ID: {}", host.build_id()));
    }
    if !host.has_objects() || host.object_count() != 15 {
        return Err("object telemetry ABI mismatch".to_owned());
    }
    if host.output_latency_samples() != 64 {
        return Err("output-latency ABI mismatch".to_owned());
    }

    let mut callbacks = 0usize;
    host.process_raw(&[0x0B, 0x77, 0x00, 0x01], |samples, frames| {
        assert_eq!(frames, 1);
        assert_eq!(samples.len(), 12);
        assert!((samples[11] - 11.0 / 16.0).abs() < 1.0e-6);
        callbacks += 1;
    })?;
    if callbacks != 1 {
        return Err(format!("expected one render callback, got {callbacks}"));
    }

    println!("liborender ABI probe: PASS");
    Ok(())
}
