#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JOCFORGE_REV="${AURORA_JOCFORGE_REV:-05a4108e0c6288130dec1203b301979a91475fca}"
WORK_ROOT="${AURORA_JOCFORGE_WORKDIR:-}"
CLEANUP_WORK_ROOT=0

if [[ -z "$WORK_ROOT" ]]; then
  WORK_ROOT="$(mktemp -d)"
  CLEANUP_WORK_ROOT=1
fi

cleanup() {
  if [[ "$CLEANUP_WORK_ROOT" -eq 1 ]]; then
    rm -rf "$WORK_ROOT"
  fi
}
trap cleanup EXIT

for tool in cargo git ffmpeg cmp grep; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "required tool missing: $tool" >&2
    exit 1
  }
done

FORGE_ROOT="$WORK_ROOT/JOCForge"
SCENE="$WORK_ROOT/aurora-jocforge-reference.bw64"
mkdir -p "$WORK_ROOT"

if [[ ! -d "$FORGE_ROOT/.git" ]]; then
  git init -q "$FORGE_ROOT"
  git -C "$FORGE_ROOT" remote add origin https://github.com/chyinan/JOCForge.git
fi

git -C "$FORGE_ROOT" fetch --quiet --depth 1 origin "$JOCFORGE_REV"
git -C "$FORGE_ROOT" checkout --quiet --detach FETCH_HEAD
ACTUAL_REV="$(git -C "$FORGE_ROOT" rev-parse HEAD)"
if [[ "$ACTUAL_REV" != "$JOCFORGE_REV" ]]; then
  echo "JOCForge revision mismatch: expected $JOCFORGE_REV got $ACTUAL_REV" >&2
  exit 1
fi

(
  cd "$FORGE_ROOT"
  cargo run --quiet -p jocforge --locked -- \
    fixture --seconds 1 "$SCENE"
)

PROFILE_IDS=(idx0 idx1 idx2 idx3 idx4)
PROFILE_ARGS=(idx0 idx1 idx2 "5.X Phase" "5.X+2 Phase")

for index in "${!PROFILE_IDS[@]}"; do
  profile_id="${PROFILE_IDS[$index]}"
  profile_arg="${PROFILE_ARGS[$index]}"
  raw="$WORK_ROOT/jocforge-${profile_id}.ec3"
  carrier="$WORK_ROOT/jocforge-${profile_id}.iec61937"
  roundtrip="$WORK_ROOT/jocforge-${profile_id}.roundtrip.ec3"
  probe_log="$WORK_ROOT/jocforge-${profile_id}.probe.log"
  latency="$WORK_ROOT/jocforge-${profile_id}.latency.tsv"

  echo "[jocforge] encode profile=$profile_id arg=$profile_arg"
  (
    cd "$FORGE_ROOT"
    cargo run --quiet -p jocforge --locked -- \
      encode "$SCENE" "$raw" --profile "$profile_arg"
  )

  test -s "$raw" || {
    echo "JOCForge produced an empty stream for $profile_id" >&2
    exit 1
  }

  (
    cd "$ROOT"
    cargo run --quiet --locked -p aurora-cli \
      --no-default-features --features earc-sim \
      --bin aurora-sim-source -- \
      < "$raw" > "$carrier"

    cargo run --quiet --locked -p aurora-cli \
      --no-default-features \
      --bin aurora-direct-earc-probe -- \
      --filter eac3 --extract eac3 \
      < "$carrier" > "$roundtrip" 2> "$probe_log"
  )

  cmp "$raw" "$roundtrip"
  grep -Eq 'eac3=[1-9][0-9]*' "$probe_log"
  grep -q 'malformed_headers=0' "$probe_log"
  grep -q 'eac3_period_mismatches=0' "$probe_log"

  (
    cd "$ROOT"
    cargo run --quiet --locked -p aurora-cli \
      --no-default-features --features validation \
      --bin aurora-sim -- \
      latency-report --input "$raw" --iterations 1 \
      > "$latency"
  )
  grep -Eq '^joc_render[[:space:]]+[1-9][0-9]*' "$latency"

done

echo "jocforge_revision=$JOCFORGE_REV"
echo "jocforge_profiles_passed=${#PROFILE_IDS[@]}"
echo "jocforge_scope=synthetic-software-validation-only"
