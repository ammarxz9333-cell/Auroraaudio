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
USB_ENCODED = 1
USB_PCM = 2
USB_CONFIG = 4
USB_PONG = 8
USB_FLAG_DISCONTINUITY = 1 << 1
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


def expect_no_packet(sock, timeout=0.06):
    old_timeout = sock.gettimeout()
    sock.settimeout(timeout)
    try:
        sock.recv(4096)
        raise AssertionError("unexpected packet reached single USB backend")
    except socket.timeout:
        pass
    finally:
        sock.settimeout(old_timeout)


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
        local_path = os.path.join(td, "local.sock")
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
        gate_env["AURORA_LOCAL_SOURCE_SOCKET"] = local_path
        gate_env["AURORA_USB_BRIDGE_SOCKET_REAL"] = bridge_path
        gate = subprocess.Popen(
            [gate_bin], env=gate_env, stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, text=True
        )

        live = None
        local = None
        bridge = None
        try:
            bridge, _ = bridge_listener.accept()
            bridge.settimeout(1)
            live = wait_connect(source_path)
            live.settimeout(1)
            local = wait_connect(local_path)
            local.settimeout(1)

            # CONFIG may pass before grant because both source adapters need to
            # establish the common 48 kHz/12-channel MCU contract.
            config_payload = bytes(range(48))
            live.sendall(frame(USB_CONFIG, config_payload, seq=1))
            forwarded = parse_frame(bridge.recv(4096))
            assert forwarded["kind"] == USB_CONFIG
            assert forwarded["payload"] == config_payload

            # Local data uses the same single backend, never a second FunctionFS
            # client. A local CONFIG must reach the same backend socket.
            local_config_payload = bytes(reversed(range(48)))
            local.sendall(frame(USB_CONFIG, local_config_payload, seq=2))
            local_forwarded = parse_frame(bridge.recv(4096))
            assert local_forwarded["kind"] == USB_CONFIG
            assert local_forwarded["payload"] == local_config_payload

            sample = 0x20000000
            pcm_payload = struct.pack("<i", sample) * (PERIOD_FRAMES * CHANNELS)

            # In a multi-source system inactive HDMI PCM must be DROPPED, not
            # replaced by zero periods that would overwrite an active local source.
            live.sendall(frame(USB_PCM, pcm_payload, seq=3,
                               aux=(CHANNELS << 16) | PERIOD_FRAMES))
            expect_no_packet(bridge)

            # Non-media MCU control traffic is duplicated to connected source
            # adapters so each can maintain independent CONFIG/drift state.
            bridge.sendall(frame(USB_PONG, b"", seq=4))
            hdmi_control = parse_frame(live.recv(4096))
            local_control = parse_frame(local.recv(4096))
            assert hdmi_control["kind"] == USB_PONG
            assert local_control["kind"] == USB_PONG

            # IEC61937 media is HDMI-only and establishes HDMI presence.
            encoded_payload = b"\x72\xf8\x1f\x4e\x15\x00\x00\x00"
            bridge.sendall(frame(USB_ENCODED, encoded_payload, seq=5, pts=100))
            encoded = parse_frame(live.recv(4096))
            assert encoded["kind"] == USB_ENCODED
            assert encoded["payload"] == encoded_payload
            expect_no_packet(local)

            time.sleep(0.05)
            peaks, flags = wait_for_audible_pcm(live, bridge, pcm_payload, 10)
            assert any(p > 0 for p in peaks), "PCM never opened after HDMI GRANT"
            assert peaks[-1] >= int(sample * 0.95), peaks[-4:]
            assert any(f & USB_FLAG_DISCONTINUITY for f in flags), \
                "first post-GRANT PCM did not mark discontinuity"

            # A physical USB session reset invalidates MCU CONFIG for every data
            # source. The gate must close BOTH upstream clients and reconnect to
            # FunctionFS with one backend only.
            bridge.close()
            bridge = None
            live.settimeout(1.0)
            local.settimeout(1.0)
            assert live.recv(1) == b"", "HDMI source stayed connected across USB reset"
            assert local.recv(1) == b"", "local source stayed connected across USB reset"
            live.close()
            local.close()
            live = None
            local = None

            bridge, _ = bridge_listener.accept()
            bridge.settimeout(1)
            live = wait_connect(source_path)
            live.settimeout(1)
            local = wait_connect(local_path)
            local.settimeout(1)

            live.sendall(frame(USB_CONFIG, config_payload, seq=200))
            fresh_config = parse_frame(bridge.recv(4096))
            assert fresh_config["kind"] == USB_CONFIG
            assert fresh_config["payload"] == config_payload

            local.sendall(frame(USB_CONFIG, local_config_payload, seq=201))
            fresh_local_config = parse_frame(bridge.recv(4096))
            assert fresh_local_config["kind"] == USB_CONFIG
            assert fresh_local_config["payload"] == local_config_payload

            bridge.sendall(frame(USB_ENCODED, encoded_payload, seq=202, pts=200))
            encoded = parse_frame(live.recv(4096))
            assert encoded["kind"] == USB_ENCODED
            time.sleep(0.10)
            peaks, flags = wait_for_audible_pcm(live, bridge, pcm_payload, 220)
            assert peaks[-1] >= int(sample * 0.95), peaks[-4:]
            assert any(f & USB_FLAG_DISCONTINUITY for f in flags), \
                "post-reset PCM did not mark discontinuity"

            # Manager loss revokes HDMI ownership. In multi-source mode fail-closed
            # means no inactive HDMI PCM reaches the backend at all.
            terminate(manager)
            manager = None
            time.sleep(0.10)
            for i in range(3):
                live.sendall(frame(USB_PCM, pcm_payload, seq=300 + i,
                                   aux=(CHANNELS << 16) | PERIOD_FRAMES))
                expect_no_packet(bridge)

        finally:
            if live is not None:
                live.close()
            if local is not None:
                local.close()
            if bridge is not None:
                bridge.close()
            bridge_listener.close()
            terminate(gate)
            terminate(manager)

            gate_stderr = gate.stderr.read() if gate.stderr else ""
            if gate.returncode not in (0, -15):
                raise AssertionError(f"source gate exited {gate.returncode}: {gate_stderr}")

    print("source-gate single-backend HDMI/local mux + reset/fail-closed tests passed")


if __name__ == "__main__":
    main()