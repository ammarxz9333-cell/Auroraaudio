use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aurora-cli"))
        .args(args)
        .output()
        .expect("run aurora-cli")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn top_level_command_surface_is_preserved() {
    let output = run(&["--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let help = stdout(&output);

    for command in [
        "devices",
        "doctor",
        "gains",
        "render",
        "evaluate-renderer",
        "process",
        "realtime",
        "duplex",
        "measure-latency",
        "duplex-soak",
        "simulate-duplex",
        "simulate-latency",
        "simulate-output-validation",
        "identify-speakers",
    ] {
        assert!(
            help.contains(command),
            "top-level help no longer contains command `{command}`:\n{help}"
        );
    }
}

#[test]
fn render_option_surface_is_preserved() {
    let output = run(&["render", "--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let help = stdout(&output);

    for option in [
        "--scene",
        "--input",
        "--output",
        "--apply-geometric-delay",
        "--speed-of-sound",
        "--renderer-mode",
    ] {
        assert!(
            help.contains(option),
            "render help no longer contains option `{option}`:\n{help}"
        );
    }
}

#[test]
fn evaluation_option_surface_is_preserved() {
    let output = run(&["evaluate-renderer", "--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let help = stdout(&output);

    for option in [
        "--scene",
        "--input",
        "--output-dir",
        "--fixture",
        "--renderer",
        "--commit-sha",
        "--max-renderer-p99-ns",
    ] {
        assert!(
            help.contains(option),
            "evaluation help no longer contains option `{option}`:\n{help}"
        );
    }
}

#[test]
fn deprecated_binaural_alias_still_parses() {
    let output = run(&[
        "render",
        "--scene",
        "does-not-exist.json",
        "--input",
        "does-not-exist.wav",
        "--output",
        "unused.wav",
        "--renderer-mode",
        "binaural",
    ]);
    let error = stderr(&output);

    assert!(
        !error.contains("invalid value 'binaural'"),
        "deprecated renderer alias stopped parsing:\n{error}"
    );
    assert!(
        error.contains("load scene") || error.contains("does-not-exist"),
        "command did not progress beyond argument parsing as expected:\n{error}"
    );
}
