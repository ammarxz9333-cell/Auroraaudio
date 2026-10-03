#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${AURORA_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
RUNTIME_ROOT="${AURORA_RUNTIME_ROOT:-$HOME/.local/share/aurora-runtime}"
PREFIX="$RUNTIME_ROOT/install"
STATE_DIR="${AURORA_STATE_DIR:-$RUNTIME_ROOT/state}"

DEVICE="${AURORA_EARC_DEVICE:-hw:eARC,0}"
ORENDER="${AURORA_ORENDER:-$PREFIX/bin/orender}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$PREFIX/lib/libharletty_bridge.so}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$PREFIX/share/aurora/omniphony-11.1.4-aurora.yaml}"
RENDER_CONFIG="${AURORA_RENDER_CONFIG:-}"

OUTPUT_MODE="${AURORA_OUTPUT_MODE:-camilladsp}"
OUTPUT_RATE="${AURORA_OUTPUT_RATE:-48000}"
MASTER_GAIN_DB="${AURORA_MASTER_GAIN_DB:--3}"
AUTO_GAIN_CEILING_DB="${AURORA_AUTO_GAIN_CEILING_DB:--1}"

CAMILLADSP="${AURORA_CAMILLADSP:-$PREFIX/bin/camilladsp}"
CAMILLADSP_CONFIG="${AURORA_CAMILLADSP_CONFIG:-$STATE_DIR/camilladsp-runtime.yml}"
CAMILLA_GEN="$ROOT_DIR/scripts/pi5/make-camilladsp-config.py"
ALSA_OUTPUT_DEVICE="${AURORA_ALSA_OUTPUT_DEVICE:-default}"
ALSA_OUTPUT_FORMAT="${AURORA_ALSA_OUTPUT_FORMAT:-auto}"
CAMILLA_CHUNK="${AURORA_CAMILLADSP_CHUNK:-512}"
CAMILLA_QUEUE="${AURORA_CAMILLADSP_QUEUELIMIT:-2}"
CAMILLA_TARGET="${AURORA_CAMILLADSP_TARGET_LEVEL:-512}"
CAMILLA_ADJUST_PERIOD="${AURORA_CAMILLADSP_ADJUST_PERIOD:-3}"

PIPEWIRE_OUTPUT_DEVICE="${AURORA_OUTPUT_DEVICE:-}"
PIPEWIRE_LATENCY_MS="${AURORA_LATENCY_MS:-80}"
PIPEWIRE_ADAPTIVE="${AURORA_ADAPTIVE_RESAMPLING:-1}"

CONVERTER="$ROOT_DIR/validation/physical/aurora_alsa_iec61937_stream.py"
mkdir -p "$STATE_DIR"

for path in "$ORENDER" "$BRIDGE" "$LAYOUT" "$CONVERTER"; do
  [[ -e "$path" ]] || { echo "missing Aurora runtime component: $path" >&2; exit 2; }
done
command -v arecord >/dev/null 2>&1 || { echo "arecord not found (install alsa-utils)" >&2; exit 2; }

GLOBAL_ARGS=()
if [[ -n "$RENDER_CONFIG" ]]; then
  [[ -f "$RENDER_CONFIG" ]] || { echo "Aurora render config not found: $RENDER_CONFIG" >&2; exit 2; }
  GLOBAL_ARGS+=(--config "$RENDER_CONFIG")
fi

RENDER_ARGS=(
  --bridge-path "$BRIDGE"
  --enable-vbap
  --speaker-layout "$LAYOUT"
  --output-sample-rate "$OUTPUT_RATE"
  --master-gain "$MASTER_GAIN_DB"
  --auto-gain
  --auto-gain-ceiling "$AUTO_GAIN_CEILING_DB"
)

echo "Aurora Pi5 eARC runtime" >&2
echo "  capture : $DEVICE (S32_LE/2ch IEC61937 carrier @ 192 kHz for DD+)" >&2
echo "  bridge  : $BRIDGE" >&2
echo "  layout  : $LAYOUT" >&2
echo "  render  : $OUTPUT_RATE Hz, master $MASTER_GAIN_DB dB, ceiling $AUTO_GAIN_CEILING_DB dBFS" >&2
echo "  output  : $OUTPUT_MODE" >&2
echo "  config  : ${RENDER_CONFIG:-built-in/CLI baseline}" >&2

case "$OUTPUT_MODE" in
  camilladsp)
    [[ -x "$CAMILLADSP" ]] || { echo "CamillaDSP not installed: $CAMILLADSP" >&2; exit 2; }
    [[ -f "$CAMILLA_GEN" ]] || { echo "CamillaDSP config generator missing: $CAMILLA_GEN" >&2; exit 2; }

    if [[ -z "${AURORA_CAMILLADSP_CONFIG:-}" ]]; then
      python3 "$CAMILLA_GEN" \
        --output "$CAMILLADSP_CONFIG" \
        --device "$ALSA_OUTPUT_DEVICE" \
        --samplerate "$OUTPUT_RATE" \
        --channels 16 \
        --chunksize "$CAMILLA_CHUNK" \
        --queuelimit "$CAMILLA_QUEUE" \
        --target-level "$CAMILLA_TARGET" \
        --adjust-period "$CAMILLA_ADJUST_PERIOD" \
        --playback-format "$ALSA_OUTPUT_FORMAT" \
        >&2
    else
      [[ -f "$CAMILLADSP_CONFIG" ]] || {
        echo "AURORA_CAMILLADSP_CONFIG not found: $CAMILLADSP_CONFIG" >&2
        exit 2
      }
    fi

    "$CAMILLADSP" --check "$CAMILLADSP_CONFIG" >/dev/null
    echo "  post-DSP: CamillaDSP 16ch -> ALSA $ALSA_OUTPUT_DEVICE" >&2
    echo "  clock   : AsyncSinc rate-adjust, chunk=$CAMILLA_CHUNK target=$CAMILLA_TARGET" >&2

    # eARC hardware pace -> decoder/object renderer -> raw interleaved 16ch F32
    # -> CamillaDSP. CamillaDSP owns final room-DSP and the independent DAC
    # clock-domain servo. pipefail propagates a failure from any stage.
    python3 "$CONVERTER" capture \
      --device "$DEVICE" \
      --iec-out - \
      --status "$STATE_DIR/earc-status.json" \
      --hw-params-log "$STATE_DIR/earc-hw-params.txt" \
      --stderr-log "$STATE_DIR/arecord.stderr.log" \
      | "$ORENDER" "${GLOBAL_ARGS[@]}" render - \
          "${RENDER_ARGS[@]}" \
          --output-backend file \
          --output-file - \
          --output-file-format raw-f32 \
      | "$CAMILLADSP" "$CAMILLADSP_CONFIG"
    ;;

  pipewire)
    OUTPUT_ARGS=(
      --output-backend pipewire
      --latency-target-ms "$PIPEWIRE_LATENCY_MS"
    )
    if [[ "$PIPEWIRE_ADAPTIVE" == "1" ]]; then
      OUTPUT_ARGS+=(--enable-adaptive-resampling)
    fi
    if [[ -n "$PIPEWIRE_OUTPUT_DEVICE" ]]; then
      OUTPUT_ARGS+=(--output-device "$PIPEWIRE_OUTPUT_DEVICE")
    fi

    echo "  fallback: PipeWire ${PIPEWIRE_OUTPUT_DEVICE:-default}, target ${PIPEWIRE_LATENCY_MS} ms" >&2
    echo "  clock   : Omniphony adaptive_resampling=$PIPEWIRE_ADAPTIVE" >&2

    python3 "$CONVERTER" capture \
      --device "$DEVICE" \
      --iec-out - \
      --status "$STATE_DIR/earc-status.json" \
      --hw-params-log "$STATE_DIR/earc-hw-params.txt" \
      --stderr-log "$STATE_DIR/arecord.stderr.log" \
      | "$ORENDER" "${GLOBAL_ARGS[@]}" render - \
          "${RENDER_ARGS[@]}" \
          "${OUTPUT_ARGS[@]}"
    ;;

  *)
    echo "unsupported AURORA_OUTPUT_MODE=$OUTPUT_MODE (use camilladsp or pipewire)" >&2
    exit 2
    ;;
esac
