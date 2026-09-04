#!/usr/bin/env python3
import os
import socket
import struct
import subprocess
import sys
import tempfile
import time

USB_MAGIC = 0x30525541  # "AUR0" on little-endian wire
USB_VERSION = 1
USB_HEADER = 32
USB_CONFIG = 4
USB_ENCODED = 1
USB_PCM = 2
USB_FLAG_DISCONTINUITY = 1 << 1
SAMPLE_RATE = 48000
PERIOD_FRAMES = 40
CHANNELS = 12
PCM_BYTES = PERIOD_FRAMES * CHANNELS * 4

HEADER = struct.Struct("<IHHIIQII")
assert HEADER.size == USB_HEADER


def frame(kind, payload=b"", flags=0, seq=1, pts=0, aux=0):
    return HEADER.pack(USB_MAGIC, USB_VERSION, kind, flags, seq, pts, len(payload), aux) + payload


def parse_frame(raw):
    if len(raw) < USB_HEADER:
        raise AssertionError(f"short Aurora frame: {len(raw)}")
    magic, version, kind, flags, seq, pts, payload_len, aux = HEADER.unpack(raw[:USB_HEADER])
    assert magic == USB_MAGIC and version == USB_VERSION
    assert len(raw) == USB_HEADER + payload_len
    return {
        "kind": kind,
        "flags": flags,
        "seq": seq,
        "pts": pts,
        "aux": aux,
        "payload": raw[USB_HEADER:],
    }


def wait_connect(path, timeout=2.0):
    deadline = time.monotonic() + timeout
    while True:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        try:
            s.connect(path)
            return s
        except (FileNotFoundError, ConnectionRefusedError):
            s.close()
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.01)


def all_pcm_zero(payload):
    return payload == b"\x00" * len(payload)


def pcm_peak(payload):
    samples = struct.unpack("<" + "i" * (len(payload) // 4), payload)
    return max(abs(v) for v in samples)


def terminate(proc):
    if proc is None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=2)


def wait_for_audible_pcm(live, bridge, pcm_payload, seq_base):
    peaks = []
    flags = []
    for i in range(16):
        live.sendall(frame(USB_PCM, pcm_payload, seq=seq_base + i,
                           aux=(CHANNELS << 16) | PERIOD_FRAMES))
        out = parse_frame(bridge.recv(USB_HEADER + PCM_BYTES))
        assert out["kind"] == USB_PCM
        peaks.append(pcm_peak(out["payload"]))
        flags.append(out["flags"])
    return peaks, flags


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: test_source_gate.py /path/to/aurora-source-manager /path/to/aurora-source-gate")
    manager_bin, gate_bin = sys.argv[1:]

    with tempfile.TemporaryDirectory(prefix="aurora-source-gate-") as td:
        manager_path = os.path.join(td, "manager.sock")
        source_path = os.path.join(td, "hdmi.sock")
        bridge_path = os.path.join(td, "bridge.sock")

        bridge_listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        bridge_listener.bind(bridge_path)
        bridge_listener.listen(1)
        bridge_listener.settimeout(2)

        manager_env = os.environ.copy()
        manager_env["AURORA_SOURCE_MANAGER_SOCKET"] = manager_path
        manager = subprocess.Popen(
            [manager_bin], env=manager_env, stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, text=True
        )

        gate_env = os.environ.copy()
        gate_env["AURORA_SOURCE_MANAGER_SOCKET"] = manager_path
        gate_env["AURORA_HDMI_SOURCE_SOCKET"] = source_path
        gate_env["AURORA_USB_BRIDGE_SOCKET_REAL"] = bridge_path
        gate = subprocess.Popen(
            [gate_bin], env=gate_env, stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, text=True
        )

        live = None
        bridge = None
        try:
            bridge, _ = bridge_listener.accept()
            bridge.settimeout(1)
            live = wait_connect(source_path)
            live.settimeout(1)

            # Control/config traffic may pass before a source grant; audio may not.
            config_payload = bytes(range(48))
            live.sendall(frame(USB_CONFIG, config_payload, seq=1))
            forwarded = parse_frame(bridge.recv(4096))
            assert forwarded["kind"] == USB_CONFIG
            assert forwarded["payload"] == config_payload

            sample = 0x20000000
            pcm_payload = struct.pack("<i", sample) * (PERIOD_FRAMES * CHANNELS)
            live.sendall(frame(USB_PCM, pcm_payload, seq=2,
                               aux=(CHANNELS << 16) | PERIOD_FRAMES))
            muted = parse_frame(bridge.recv(USB_HEADER + PCM_BYTES))
            assert muted["kind"] == USB_PCM
            assert all_pcm_zero(muted["payload"]), "PCM leaked before source GRANT"

            # Encoded traffic establishes HDMI presence. It must remain byte-exact.
            encoded_payload = b"\x72\xf8\x1f\x4e\x15\x00\x00\x00"
            bridge.sendall(frame(USB_ENCODED, encoded_payload, seq=3, pts=100))
            encoded = parse_frame(live.recv(4096))
            assert encoded["kind"] == USB_ENCODED
            assert encoded["payload"] == encoded_payload

            time.sleep(0.05)
            peaks, flags = wait_for_audible_pcm(live, bridge, pcm_payload, 10)
            assert any(p > 0 for p in peaks), "PCM never opened after HDMI GRANT"
            assert peaks[-1] >= int(sample * 0.95), peaks[-4:]
            assert any(f & USB_FLAG_DISCONTINUITY for f in flags), \
                "first post-GRANT PCM did not mark discontinuity"

            # A physical USB session reset invalidates MCU CONFIG. FunctionFS
            # therefore drops the real backend. The gate must propagate that
            # reset upstream by closing the live source socket; otherwise the
            # broker would incorrectly keep its old configured=true state.
            bridge.close()
            bridge = None
            live.settimeout(1.0)
            assert live.recv(1) == b"", \
                "upstream source stayed connected across USB backend reset"
            live.close()
            live = None

            # The gate reconnects to the new FunctionFS backend, and the live
            # broker side must reconnect separately and start with a fresh CONFIG.
            bridge, _ = bridge_listener.accept()
            bridge.settimeout(1)
            live = wait_connect(source_path)
            live.settimeout(1)
            live.sendall(frame(USB_CONFIG, config_payload, seq=200))
            fresh_config = parse_frame(bridge.recv(4096))
            assert fresh_config["kind"] == USB_CONFIG
            assert fresh_config["payload"] == config_payload

            # Re-establish HDMI presence/grant after the reset and prove audible
            # output returns only through the new session.
            bridge.sendall(frame(USB_ENCODED, encoded_payload, seq=201, pts=200))
            encoded = parse_frame(live.recv(4096))
            assert encoded["kind"] == USB_ENCODED
            time.sleep(0.10)
            peaks, flags = wait_for_audible_pcm(live, bridge, pcm_payload, 220)
            assert peaks[-1] >= int(sample * 0.95), peaks[-4:]
            assert any(f & USB_FLAG_DISCONTINUITY for f in flags), \
                "post-reset PCM did not mark discontinuity"

            # Killing the manager must immediately fail closed. The gate may reconnect
            # later, but until a fresh GRANT arrives no audible PCM may escape.
            terminate(manager)
            manager = None
            time.sleep(0.10)
            for i in range(3):
                live.sendall(frame(USB_PCM, pcm_payload, seq=300 + i,
                                   aux=(CHANNELS << 16) | PERIOD_FRAMES))
                out = parse_frame(bridge.recv(USB_HEADER + PCM_BYTES))
                assert all_pcm_zero(out["payload"]), "PCM leaked after manager loss"

        finally:
            if live is not None:
                live.close()
            if bridge is not None:
                bridge.close()
            bridge_listener.close()
            terminate(gate)
            terminate(manager)

            gate_stderr = gate.stderr.read() if gate.stderr else ""
            if gate.returncode not in (0, -15):
                raise AssertionError(f"source gate exited {gate.returncode}: {gate_stderr}")

    print("source-gate fail-closed/ramp/backend-reset/manager-loss integration tests passed")


if __name__ == "__main__":
    main()