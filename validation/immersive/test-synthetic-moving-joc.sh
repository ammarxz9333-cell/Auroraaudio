#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 EXTERNAL_ENCODER_REPO [OUTPUT_DIR]" >&2
  echo "EXTERNAL_ENCODER_REPO must be a clean checkout of the pinned research encoder commit." >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENCODER_REPO="$(cd "$1" && pwd)"
OUTPUT_DIR=${2:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-synthetic-moving-joc.XXXXXX")"}
GENERATOR="$ROOT_DIR/validation/immersive/generate-synthetic-moving-damf.py"
TEMPORAL_HARNESS="$ROOT_DIR/validation/immersive/test-joc-temporal-evidence.sh"
OPENJOC_INSTALLER="$ROOT_DIR/validation/immersive/install-openjoc-reference.sh"
PINNED_ENCODER_COMMIT="faf3ef16c48dca52958f3cd1276a9796477eba1d"

fail() {
  echo "AURORA-SYNTHETIC-MOVING-JOC-FAIL: $*" >&2
  exit 2
}

for cmd in cargo ffmpeg git python3 sha256sum; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
[[ -f "$GENERATOR" ]] || fail "generator missing: $GENERATOR"
[[ -f "$TEMPORAL_HARNESS" ]] || fail "temporal harness missing: $TEMPORAL_HARNESS"
[[ -f "$OPENJOC_INSTALLER" ]] || fail "OpenJOC installer missing: $OPENJOC_INSTALLER"
[[ -f "$ENCODER_REPO/Cargo.toml" ]] || fail "not an encoder checkout: $ENCODER_REPO"

ENCODER_HEAD="$(git -C "$ENCODER_REPO" rev-parse HEAD)"
[[ "$ENCODER_HEAD" == "$PINNED_ENCODER_COMMIT" ]] || \
  fail "external encoder must be pinned at $PINNED_ENCODER_COMMIT, got $ENCODER_HEAD"
[[ -z "$(git -C "$ENCODER_REPO" status --porcelain)" ]] || \
  fail "external encoder checkout must be clean"

mkdir -p "$OUTPUT_DIR"
DAMF_DIR="$OUTPUT_DIR/damf"
CORE="$OUTPUT_DIR/synthetic-core.eac3"
CARRIER="$OUTPUT_DIR/synthetic-moving-joc.eac3"
TEMPORAL_DIR="$OUTPUT_DIR/temporal"
SOURCE_REPORT="$DAMF_DIR/aurora-moving-source.json"

printf '\n== Aurora synthetic moving-JOC phase: generate owned DAMF source ==\n'
python3 "$GENERATOR" "$DAMF_DIR"
MANIFEST="$DAMF_DIR/aurora-moving.atmos"
[[ -s "$MANIFEST" && -s "$SOURCE_REPORT" ]] || fail "synthetic DAMF generation failed"

printf '\n== Aurora synthetic moving-JOC phase: build pinned external research encoder ==\n'
cargo build --release --locked --manifest-path "$ENCODER_REPO/Cargo.toml"
ENCODER_BIN="$ENCODER_REPO/target/release/dolby-atmos-encoder"
[[ -x "$ENCODER_BIN" ]] || fail "encoder binary missing after build: $ENCODER_BIN"
"$ENCODER_BIN" --version
"$ENCODER_BIN" inspect "$MANIFEST"

printf '\n== Aurora synthetic moving-JOC phase: construct 5.1 E-AC-3 core ==\n'
"$ENCODER_BIN" downmix "$MANIFEST" --out - \
  | ffmpeg -hide_banner -loglevel error -y \
      -f f32le -ar 48000 -ac 6 -i - \
      -c:a eac3 -b:a 768k -f eac3 "$CORE"
[[ -s "$CORE" ]] || fail "E-AC-3 core was not produced"

printf '\n== Aurora synthetic moving-JOC phase: inject moving OAMD/JOC ==\n'
"$ENCODER_BIN" atmos "$CORE" "$MANIFEST" --out "$CARRIER"
[[ -s "$CARRIER" ]] || fail "synthetic JOC carrier was not produced"
CARRIER_SHA256="$(sha256sum "$CARRIER" | awk '{print $1}')"
printf '%s  %s\n' "$CARRIER_SHA256" "$(basename "$CARRIER")" >"$OUTPUT_DIR/synthetic-moving-joc.sha256"

if [[ -z "${OPENJOC_BIN:-}" ]]; then
  printf '\n== Aurora synthetic moving-JOC phase: install pinned OpenJOC reference ==\n'
  OPENJOC_BIN="$(bash "$OPENJOC_INSTALLER" "$OUTPUT_DIR/openjoc-reference")"
  export OPENJOC_BIN
fi

PROVENANCE="Aurora-owned deterministic synthetic DAMF v1; source report=$SOURCE_REPORT; external research encoder raress96/dolby-atmos-encoder@$PINNED_ENCODER_COMMIT; software interoperability exercise only; not Dolby-authored, Dolby-certified, protected-streaming, or physical-hardware evidence"

printf '\n== Aurora synthetic moving-JOC phase: unchanged temporal harness ==\n'
set +e
bash "$TEMPORAL_HARNESS" \
  "$CARRIER" \
  "$CARRIER_SHA256" \
  "$PROVENANCE" \
  "$TEMPORAL_DIR"
RC=$?
set -e

cat >"$OUTPUT_DIR/synthetic-moving-joc-provenance.txt" <<EOF
source=org.aurora.synthetic-moving-damf-source.v1
source_report=$SOURCE_REPORT
external_encoder_repo=https://github.com/raress96/dolby-atmos-encoder
external_encoder_commit=$PINNED_ENCODER_COMMIT
external_encoder_checkout=$ENCODER_REPO
carrier=$CARRIER
carrier_sha256=$CARRIER_SHA256
temporal_harness_exit=$RC
claim_scope=diagnostic generated-carrier software evidence only; not genuine Dolby-authored moving-object proof and not Dolby hardware/certification evidence
EOF

if [[ "$RC" == "0" ]]; then
  echo "AURORA-SYNTHETIC-MOVING-JOC-PASS"
  echo "This proves the fail-closed Aurora/OpenJOC temporal lane can admit a generated moving-object carrier."
  echo "It does NOT upgrade Aurora's claim to genuine Dolby-authored moving-object JOC proof."
  echo "evidence: $TEMPORAL_DIR/joc-temporal-evidence.json"
  exit 0
fi
if [[ "$RC" == "3" ]]; then
  echo "AURORA-SYNTHETIC-MOVING-JOC-INSUFFICIENT: generated carrier did not establish required temporal diversity" >&2
  exit 3
fi

echo "AURORA-SYNTHETIC-MOVING-JOC-REJECTED: generated carrier failed codec/contract/shape admission" >&2
exit "$RC"
