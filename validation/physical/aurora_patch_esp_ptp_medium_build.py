#!/usr/bin/env python3
"""Gate exact-pin ESP-PTP Wi-Fi code on configured Wi-Fi media.

Apply before the separate clock-evidence patch. No hardware-clock, Ethernet,
servo or evidence logic is replaced. An unconfigured Wi-Fi start fails closed.
"""

import argparse
from pathlib import Path
import subprocess

PIN = "5b7eec233a93733ae954beefb6df3bb9c12dc901"
GUARD = ("#if defined(CONFIG_ESP_PTP_PORT0_MEDIUM_WIFI_FTM) || "
         "defined(CONFIG_ESP_PTP_PORT1_MEDIUM_WIFI_FTM)\n")


def replace(text, old, new):
    if text.count(old) != 1:
        raise ValueError(f"expected one source anchor: {old!r}")
    return text.replace(old, new, 1)


def transform(name, text):
    if name == "ptp_wifi.c":
        return replace(text, '#include "sdkconfig.h"\n',
                       '#include "sdkconfig.h"\n\n' + GUARD) + "\n#endif /* configured Wi-Fi medium */\n"
    text = replace(text, "static void ptp_wifi_event_handler(",
                   GUARD + "static void ptp_wifi_event_handler(")
    text = replace(text, "static void ptp_reset_for_profile(FAR struct ptp_state_s *state) {",
                   "#endif /* configured Wi-Fi medium */\n\nstatic void ptp_reset_for_profile(FAR struct ptp_state_s *state) {")
    start = "  (void)interface; /* label only on this medium — no socket to bind */"
    text = replace(text, start, GUARD + start)
    # Bound the initializer using the next function, without touching Ethernet.
    begin = text.index("static int ptp_port_init_wifi_ftm(")
    finish = text.index("\n  return OK;\n}", begin)
    text = text[:finish] + text[finish:].replace(
        "\n  return OK;\n}",
        "\n  return OK;\n#else\n  (void)state;\n  (void)port_index;\n"
        "  (void)interface;\n  return ERROR; /* Wi-Fi not configured: never report success. */\n#endif\n}", 1)
    call = "    ptp_wifi_ap_send_announce(p, port->intf_hw_addr, &msg, sizeof(msg));"
    return replace(text, call, GUARD + call + "\n#endif")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    def git(*argv):
        return subprocess.check_output(["git", "-C", str(args.root), *argv]).decode("utf-8")
    if git("rev-parse", "HEAD").strip() != PIN:
        raise ValueError("ESP-PTP HEAD is not the exact approved pin")
    updates = {}
    for name in ("ptp.c", "ptp_wifi.c"):
        original = git("show", f"{PIN}:{name}")
        path = args.root / name
        if path.read_text(encoding="utf-8") != original:
            raise ValueError(f"{name}: source differs from exact pin; apply medium patch first")
        updates[path] = transform(name, original)
    for path, text in updates.items():
        path.write_text(text, encoding="utf-8")
    print(f"aurora-esp-ptp-medium-build: PASS pin={PIN}; wired build excludes unconfigured Wi-Fi")


if __name__ == "__main__":
    main()
