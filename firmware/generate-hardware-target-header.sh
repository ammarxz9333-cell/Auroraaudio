#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
MANIFEST="${AURORA_HW_MANIFEST:-$ROOT/config/aurora-hardware-target.env}"
OUT="${1:-/dev/stdout}"

[ -f "$MANIFEST" ] || {
    echo "hardware-target-header: manifest missing: $MANIFEST" >&2
    exit 1
}

# shellcheck disable=SC1090
. "$MANIFEST"

require_var() {
    eval "value=\${$1-}"
    [ -n "$value" ] || {
        echo "hardware-target-header: missing $1" >&2
        exit 1
    }
}

pin_port() {
    pin="$1"
    case "$pin" in
        P[A-K][0-9]|P[A-K][0-9][0-9]) ;;
        *) echo "hardware-target-header: invalid GPIO name: $pin" >&2; exit 1 ;;
    esac
    rest="${pin#P}"
    printf '%s' "$(printf '%s' "$rest" | cut -c1)"
}

pin_number() {
    pin="$1"
    rest="${pin#P}"
    printf '%s' "$(printf '%s' "$rest" | cut -c2-)"
}

emit_signal() {
    role="$1"
    pin_var="AURORA_PIN_${role}"
    ball_var="AURORA_BALL_${role}"
    af_var="AURORA_AF_${role}"

    require_var "$pin_var"
    require_var "$ball_var"
    eval "pin=\${$pin_var}"
    eval "ball=\${$ball_var}"
    port="$(pin_port "$pin")"
    number="$(pin_number "$pin")"

    printf '#define AURORA_HW_%s_PIN_NAME "%s"\n' "$role" "$pin"
    printf '#define AURORA_HW_%s_PORT_LETTER '\''%s'\''\n' "$role" "$port"
    printf '#define AURORA_HW_%s_PIN_NUMBER %su\n' "$role" "$number"
    printf '#define AURORA_HW_%s_BALL "%s"\n' "$role" "$ball"

    eval "af=\${$af_var-}"
    if [ -n "$af" ]; then
        printf '#define AURORA_HW_%s_AF %su\n' "$role" "$af"
    fi
}

for v in \
    AURORA_REALTIME_MCU_ROLE \
    AURORA_REALTIME_MCU_VENDOR \
    AURORA_REALTIME_MCU_FAMILY \
    AURORA_REALTIME_MCU_PART \
    AURORA_REALTIME_MCU_PACKAGE \
    AURORA_REALTIME_MCU_USB_HS_PHY \
    AURORA_REALTIME_MCU_SAMPLE_RATE \
    AURORA_REALTIME_MCU_CHANNELS \
    AURORA_REALTIME_MCU_PERIOD_FRAMES \
    AURORA_USB_PHY_VENDOR \
    AURORA_USB_PHY_PART \
    AURORA_USB_PHY_PACKAGE \
    AURORA_USB_PHY_PORT_MODE \
    AURORA_USB_PHY_CDP_CURRENT_MA \
    AURORA_USB_PHY_ULPI_CLOCK_HZ \
    AURORA_USB_PHY_REFERENCE_HZ \
    AURORA_USB_PHY_REFSEL \
    AURORA_USB_VBUS_SWITCH_VENDOR \
    AURORA_USB_VBUS_SWITCH_PART \
    AURORA_USB_VBUS_TARGET_MA \
    AURORA_EARC_SAI_INSTANCE \
    AURORA_EARC_SAI_BLOCK \
    AURORA_TDM_SAI_INSTANCE \
    AURORA_TDM_SAI_BLOCK
do
    require_var "$v"
done

{
    cat <<'HEADER'
/* AUTO-GENERATED. DO NOT EDIT.
 * Source of truth: config/aurora-hardware-target.env
 * Regenerate with firmware/generate-hardware-target-header.sh.
 */
#ifndef AURORA_HW_TARGET_GENERATED_H
#define AURORA_HW_TARGET_GENERATED_H
HEADER

    printf '#define AURORA_HW_MCU_ROLE "%s"\n' "$AURORA_REALTIME_MCU_ROLE"
    printf '#define AURORA_HW_MCU_VENDOR "%s"\n' "$AURORA_REALTIME_MCU_VENDOR"
    printf '#define AURORA_HW_MCU_FAMILY "%s"\n' "$AURORA_REALTIME_MCU_FAMILY"
    printf '#define AURORA_HW_MCU_PART "%s"\n' "$AURORA_REALTIME_MCU_PART"
    printf '#define AURORA_HW_MCU_PACKAGE "%s"\n' "$AURORA_REALTIME_MCU_PACKAGE"
    printf '#define AURORA_HW_USB_HS_PHY "%s"\n' "$AURORA_REALTIME_MCU_USB_HS_PHY"
    printf '#define AURORA_HW_SAMPLE_RATE_HZ %su\n' "$AURORA_REALTIME_MCU_SAMPLE_RATE"
    printf '#define AURORA_HW_CHANNELS %su\n' "$AURORA_REALTIME_MCU_CHANNELS"
    printf '#define AURORA_HW_PERIOD_FRAMES %su\n' "$AURORA_REALTIME_MCU_PERIOD_FRAMES"

    printf '#define AURORA_HW_USB_PHY_VENDOR "%s"\n' "$AURORA_USB_PHY_VENDOR"
    printf '#define AURORA_HW_USB_PHY_PART "%s"\n' "$AURORA_USB_PHY_PART"
    printf '#define AURORA_HW_USB_PHY_PACKAGE "%s"\n' "$AURORA_USB_PHY_PACKAGE"
    printf '#define AURORA_HW_USB_PHY_PORT_MODE "%s"\n' "$AURORA_USB_PHY_PORT_MODE"
    printf '#define AURORA_HW_USB_PHY_CDP_CURRENT_MA %su\n' "$AURORA_USB_PHY_CDP_CURRENT_MA"
    printf '#define AURORA_HW_USB_PHY_ULPI_CLOCK_HZ %su\n' "$AURORA_USB_PHY_ULPI_CLOCK_HZ"
    printf '#define AURORA_HW_USB_PHY_REFERENCE_HZ %su\n' "$AURORA_USB_PHY_REFERENCE_HZ"
    printf '#define AURORA_HW_USB_PHY_REFSEL 0%su\n' "$AURORA_USB_PHY_REFSEL"
    printf '#define AURORA_HW_USB_VBUS_SWITCH_VENDOR "%s"\n' "$AURORA_USB_VBUS_SWITCH_VENDOR"
    printf '#define AURORA_HW_USB_VBUS_SWITCH_PART "%s"\n' "$AURORA_USB_VBUS_SWITCH_PART"
    printf '#define AURORA_HW_USB_VBUS_TARGET_MA %su\n' "$AURORA_USB_VBUS_TARGET_MA"

    printf '#define AURORA_HW_EARC_SAI_INSTANCE "%s"\n' "$AURORA_EARC_SAI_INSTANCE"
    printf '#define AURORA_HW_EARC_SAI_BLOCK '\''%s'\''\n' "$AURORA_EARC_SAI_BLOCK"
    printf '#define AURORA_HW_TDM_SAI_INSTANCE "%s"\n' "$AURORA_TDM_SAI_INSTANCE"
    printf '#define AURORA_HW_TDM_SAI_BLOCK '\''%s'\''\n' "$AURORA_TDM_SAI_BLOCK"

    for role in \
        ULPI_STP ULPI_DIR ULPI_NXT ULPI_CLK \
        ULPI_D0 ULPI_D1 ULPI_D2 ULPI_D3 ULPI_D4 ULPI_D5 ULPI_D6 ULPI_D7 \
        EARC_FS EARC_SCK EARC_SD \
        TDM_FS TDM_SCK TDM_SD TDM_MCLK \
        USB_PHY_RESET USB_VBUS_FAULT AMP_MUTE
    do
        emit_signal "$role"
    done

    cat <<'FOOTER'
#endif /* AURORA_HW_TARGET_GENERATED_H */
FOOTER
} > "$OUT"
