#include "aurora_hw_target_generated.h"

/*
 * Compile-time bridge between the machine-readable hardware manifest and the
 * future vendor HAL translation unit. No concrete part/pin value is copied
 * here; changing the manifest regenerates this contract automatically.
 */
_Static_assert(AURORA_HW_SAMPLE_RATE_HZ == 48000u,
               "Aurora realtime target must remain 48 kHz");
_Static_assert(AURORA_HW_CHANNELS == 12u,
               "Aurora v1 realtime target must expose 12 channels");
_Static_assert(AURORA_HW_PERIOD_FRAMES == 40u,
               "Aurora realtime target must remain a 40-frame quantum");
_Static_assert(AURORA_HW_USB_PHY_CDP_CURRENT_MA >= 1500u,
               "S6 USB data path must provide at least 1.5 A CDP charging");
_Static_assert(AURORA_HW_USB_PHY_ULPI_CLOCK_HZ == 60000000u,
               "ULPI interface must run at 60 MHz");
_Static_assert(AURORA_HW_USB_PHY_REFERENCE_HZ == 24000000u,
               "selected PHY contract uses a 24 MHz reference");
_Static_assert(AURORA_HW_USB_PHY_REFSEL_VALUE == 7u,
               "24 MHz PHY reference requires REFSEL binary 111");
_Static_assert(AURORA_HW_USB_VBUS_TARGET_MA >= 1500u,
               "protected VBUS path must be sized for the CDP target");

_Static_assert(AURORA_HW_ULPI_STP_AF == 10u &&
               AURORA_HW_ULPI_DIR_AF == 10u &&
               AURORA_HW_ULPI_NXT_AF == 10u &&
               AURORA_HW_ULPI_CLK_AF == 10u,
               "ULPI alternate-function contract drifted");
_Static_assert(AURORA_HW_ULPI_D0_AF == 10u &&
               AURORA_HW_ULPI_D1_AF == 10u &&
               AURORA_HW_ULPI_D2_AF == 10u &&
               AURORA_HW_ULPI_D3_AF == 10u &&
               AURORA_HW_ULPI_D4_AF == 10u &&
               AURORA_HW_ULPI_D5_AF == 10u &&
               AURORA_HW_ULPI_D6_AF == 10u &&
               AURORA_HW_ULPI_D7_AF == 10u,
               "ULPI data alternate-function contract drifted");

_Static_assert(AURORA_HW_EARC_FS_AF == 6u &&
               AURORA_HW_EARC_SCK_AF == 6u &&
               AURORA_HW_EARC_SD_AF == 6u,
               "eARC SAI alternate-function contract drifted");
_Static_assert(AURORA_HW_TDM_FS_AF == 10u &&
               AURORA_HW_TDM_SCK_AF == 10u &&
               AURORA_HW_TDM_SD_AF == 10u &&
               AURORA_HW_TDM_MCLK_AF == 10u,
               "TDM SAI alternate-function contract drifted");

int aurora_target_contract_compiles(void)
{
    return 0;
}
