#!/usr/bin/env bash
set -euo pipefail

# Exact-head Linux validation entry point for Aurora direct eARC.
#
# This intentionally mirrors the meaningful gates from the hosted validation and
# direct-eARC workflows so the branch can be validated on any real Linux host
# when GitHub-hosted runners are unavailable. It does not claim physical eARC,
# ALSA XRUN, TDM speaker order, acoustic latency, or commercial streaming JOC.

OPENJOC_REV="e7e03bc834ac0483770933cdc50ac058b100d1e2"
OPENJOC_FIXTURE_BLOB="4a47d79c1af717c7007e0398d01266867a3a9a49"
OPENJOC_FIXTURE_SHA256="54b48754b915cef97c13752de5eace4a219da6599cdfcf26f92b5b6fffc6e3e4"
FULL_SOAK="${AURORA_FULL_SOAK:-0}"

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || fail "required tool '$1' is not installed"
}

for tool in git cargo rustc curl sha256sum grep awk cmp ffmpeg pkg-config; do
  require_tool "$tool"
done

[[ "$(uname -s)" == "Linux" ]] || fail "this exact validation entry point requires Linux"
pkg-config --exists alsa || fail "ALSA development files are missing (pkg-config cannot resolve 'alsa')"

RUSTC_VERSION="$(rustc --version | awk '{print $2}')"
CARGO_VERSION="$(cargo --version | awk '{print $2}')"
[[ "$RUSTC_VERSION" == 1.85.* ]] || \
  fail "exact validation requires rustc 1.85.x; found $RUSTC_VERSION"
[[ "$CARGO_VERSION" == 1.85.* ]] || \
  fail "exact validation requires cargo 1.85.x; found $CARGO_VERSION"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
HEAD_SHA="$(git rev-parse HEAD)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

printf 'Aurora direct-eARC validation\n'
printf 'head=%s\n' "$HEAD_SHA"
printf 'rustc=%s\n' "$(rustc --version)"
printf 'cargo=%s\n' "$(cargo --version)"
printf 'ffmpeg=%s\n' "$(ffmpeg -version | head -n 1)"

printf '\n== Resolve exact workspace graph ==\n'
cargo generate-lockfile
cargo metadata --format-version 1 --locked > "$TMP_DIR/metadata.json"

printf '\n== Linux dependency graph ==\n'
cargo tree --locked --target x86_64-unknown-linux-gnu -i alsa-sys
cargo tree --locked --target x86_64-unknown-linux-gnu -p aurora-realtime-audio-cpal
if cargo tree --locked --target x86_64-unknown-linux-gnu \
    -p aurora-realtime-audio-cpal --prefix none | grep -Eq '^cpal v[0-9]'; then
  fail "CPAL unexpectedly entered the Linux direct-eARC dependency graph"
fi

printf '\n== Formatting ==\n'
cargo fmt --all -- --check

printf '\n== Workspace tests ==\n'
cargo test --locked --workspace

printf '\n== Direct-eARC crate clippy gates ==\n'
for package in \
  aurora-iec61937 \
  aurora-sim-source \
  aurora-decoder-open \
  aurora-decoder-engine \
  aurora-direct-earc-decoder \
  aurora-encoded-input \
  aurora-encoded-runtime \
  aurora-alsa-input \
  aurora-alsa-output \
  aurora-realtime-audio-cpal; do
  cargo clippy --locked -p "$package" --all-targets -- -D warnings
done

printf '\n== Direct-eARC binaries ==\n'
cargo check --locked -p aurora-cli --bin aurora-direct-earc-ingest
cargo check --locked -p aurora-cli --bin aurora-direct-earc-probe
cargo check --locked -p aurora-cli --no-default-features --features earc-sim --bin aurora-sim-source
cargo check --locked -p aurora-cli --features encoded-runtime --bin aurora-encoded-runtime

cargo test --locked -p aurora-cli --bin aurora-direct-earc-ingest
cargo test --locked -p aurora-cli --bin aurora-direct-earc-probe
cargo test --locked -p aurora-cli --no-default-features --features earc-sim --bin aurora-sim-source
cargo test --locked -p aurora-cli --features encoded-runtime --bin aurora-encoded-runtime

cargo clippy --locked -p aurora-cli --bin aurora-direct-earc-ingest -- -D warnings
cargo clippy --locked -p aurora-cli --bin aurora-direct-earc-probe -- -D warnings
cargo clippy --locked -p aurora-cli --no-default-features --features earc-sim --bin aurora-sim-source -- -D warnings
cargo clippy --locked -p aurora-cli --features encoded-runtime --bin aurora-encoded-runtime -- -D warnings

printf '\n== Validation binary check/test/clippy ==\n'
cargo check --locked -p aurora-cli --no-default-features --features validation --bin aurora-sim
cargo test --locked -p aurora-cli --no-default-features --features validation --bin aurora-sim
cargo clippy --locked -p aurora-cli --no-default-features --features validation --bin aurora-sim -- -D warnings

printf '\n== Headless smoke ==\n'
cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features validation \
  --bin aurora-sim -- \
  channel-id --output "$TMP_DIR/aurora-channel-id.wav" --seconds-per-channel 0.005

cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features validation \
  --bin aurora-sim -- \
  latency-report --seconds 0.128 --iterations 1

printf '\n== Ordinary E-AC-3 carrier rates ==\n'
for bitrate in 384 640 768; do
  cargo run --quiet --locked -p aurora-cli \
    --no-default-features --features earc-sim \
    --bin aurora-sim-source -- \
    --generate-seconds 0.256 --bitrate-kbps "$bitrate" \
    > "$TMP_DIR/generated-${bitrate}.iec61937"

  cargo run --quiet --locked -p aurora-cli \
    --no-default-features \
    --bin aurora-direct-earc-probe -- \
    --filter eac3 \
    < "$TMP_DIR/generated-${bitrate}.iec61937" \
    > /dev/null 2> "$TMP_DIR/generated-${bitrate}.log"

  grep -Eq 'eac3=[1-9][0-9]*' "$TMP_DIR/generated-${bitrate}.log"
  grep -q 'malformed_headers=0' "$TMP_DIR/generated-${bitrate}.log"
  grep -q 'eac3_period_mismatches=0' "$TMP_DIR/generated-${bitrate}.log"
done

printf '\n== Pinned synthetic OpenJOC fixture ==\n'
FIXTURE="$TMP_DIR/openjoc-joc.ec3"
curl --fail --location --retry 3 \
  --output "$FIXTURE" \
  "https://raw.githubusercontent.com/chyinan/OpenJOC/${OPENJOC_REV}/crates/openjoc-wasm/testdata/joc.ec3"
ACTUAL_BLOB="$(git hash-object "$FIXTURE")"
[[ "$ACTUAL_BLOB" == "$OPENJOC_FIXTURE_BLOB" ]] || \
  fail "OpenJOC fixture Git blob mismatch: expected $OPENJOC_FIXTURE_BLOB got $ACTUAL_BLOB"
ACTUAL_SHA256="$(sha256sum "$FIXTURE" | awk '{print $1}')"
[[ "$ACTUAL_SHA256" == "$OPENJOC_FIXTURE_SHA256" ]] || \
  fail "OpenJOC fixture SHA-256 mismatch: expected $OPENJOC_FIXTURE_SHA256 got $ACTUAL_SHA256"

printf '\n== Synthetic JOC transport round-trip/fault gates ==\n'
cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-sim-source \
  < "$FIXTURE" > "$TMP_DIR/openjoc-joc.iec61937"
cargo run --quiet --locked -p aurora-cli \
  --no-default-features \
  --bin aurora-direct-earc-probe -- \
  --filter eac3 --extract eac3 \
  < "$TMP_DIR/openjoc-joc.iec61937" > "$TMP_DIR/openjoc-joc.roundtrip.ec3"
cmp "$FIXTURE" "$TMP_DIR/openjoc-joc.roundtrip.ec3"

cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-sim-source -- \
  --cadence-jitter 1 \
  < "$FIXTURE" > "$TMP_DIR/openjoc-joc.jitter.iec61937"
cargo run --quiet --locked -p aurora-cli \
  --no-default-features \
  --bin aurora-direct-earc-probe -- \
  --filter eac3 \
  < "$TMP_DIR/openjoc-joc.jitter.iec61937" \
  > /dev/null 2> "$TMP_DIR/openjoc-joc.jitter.log"
grep -q 'eac3_max_spacing_bytes=Some(25344)' "$TMP_DIR/openjoc-joc.jitter.log"
grep -q 'eac3_period_mismatches=4' "$TMP_DIR/openjoc-joc.jitter.log"

cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-sim-source -- \
  --truncated-eof \
  < "$FIXTURE" > "$TMP_DIR/openjoc-joc.truncated.iec61937"
if cargo run --quiet --locked -p aurora-cli \
    --no-default-features \
    --bin aurora-direct-earc-probe -- \
    --filter eac3 \
    < "$TMP_DIR/openjoc-joc.truncated.iec61937" \
    > /dev/null 2> "$TMP_DIR/openjoc-joc.truncated.log"; then
  fail "truncated EOF unexpectedly passed IEC61937 finite validation"
fi
grep -q 'incomplete burst' "$TMP_DIR/openjoc-joc.truncated.log"

printf '\n== Synthetic JOC decode/render gates ==\n'
AURORA_OPENJOC_SYNTHETIC_FIXTURE="$FIXTURE" \
  cargo test --locked -p aurora-decoder-open \
  --test openjoc_synthetic_fixture \
  synthetic_openjoc_fixture_renders_aurora_7_1_4 \
  -- --ignored --exact

AURORA_OPENJOC_SYNTHETIC_FIXTURE="$FIXTURE" \
  cargo test --locked -p aurora-direct-earc-decoder \
  --test openjoc_iec_fixture \
  synthetic_joc_survives_full_direct_earc_iec61937_chain \
  -- --ignored --exact

cargo run --quiet --locked -p aurora-cli \
  --no-default-features --features validation \
  --bin aurora-sim -- \
  latency-report --input "$FIXTURE" --iterations 1 \
  > "$TMP_DIR/openjoc-latency.tsv"
grep -Eq '^joc_render[[:space:]]+[1-9][0-9]*' "$TMP_DIR/openjoc-latency.tsv"

printf '\n== Stress ==\n'
if [[ "$FULL_SOAK" == "1" ]]; then
  printf 'Running full 1800-second acceptance soak because AURORA_FULL_SOAK=1.\n'
  cargo run --quiet --locked -p aurora-cli \
    --no-default-features --features validation \
    --bin aurora-sim -- \
    stress --switches 100 --pause-resumes 1000 \
    --duration-seconds 1800 --inject-every 257 --memory-growth-percent 5
else
  cargo run --quiet --locked -p aurora-cli \
    --no-default-features --features validation \
    --bin aurora-sim -- \
    stress --switches 4 --pause-resumes 8 \
    --duration-seconds 1 --inject-every 8 \
    --memory-growth-percent 5 --unpaced
fi

printf '\n== Committed lockfile gate ==\n'
if ! git diff --quiet -- Cargo.lock; then
  git diff --stat -- Cargo.lock >&2
  printf '\nCargo.lock is stale. All preceding gates ran against the resolver-generated lockfile.\n' >&2
  printf 'Review and commit the Cargo.lock produced by this real Cargo resolver, then rerun this script.\n' >&2
  exit 2
fi

printf '\nPASS exact-head direct-eARC software validation head=%s\n' "$HEAD_SHA"
if [[ "$FULL_SOAK" != "1" ]]; then
  printf 'NOTE full 30-minute stress remains pending; rerun with AURORA_FULL_SOAK=1 for that gate.\n'
fi
printf 'NOTE physical eARC/ALSA/TDM/XRUN/acoustic/commercial-streaming acceptance remains separate.\n'
