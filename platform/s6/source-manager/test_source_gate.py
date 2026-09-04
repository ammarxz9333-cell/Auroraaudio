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
USB_ACK = 5
USB_PONG = 8
USB_FLAG_DISCONTINUITY = 1 << 1
SAMPLE_RATE = 48000
PERIOD_FRAMES = 40
CHANNELS = 12
PCM_BYTES = PERIOD_FRAMES * CHANNELS * 4
LAYOUT_HASH = bytes.fromhex(
    "05063560d6c5c1b7d3709656cd8c644a"
    "6d2b52f5e81383771f26323442d0a244"
)

HEADER = struct.Struct("<IHHIIQII")
assert HEADER.size == USB_HEADER


def frame(kind, payload=b"", flags=0, seq=1, pts=0, aux=0):
    return HEADER.pack(
        USB_MAGIC, USB_VERSION, kind, flags, seq, pts, len(payload), aux
    ) + payload


def config_frame(seq):
    payload = bytearray(48)
    struct.pack_into("<I", payload, 0, SAMPLE_RATE)
    struct.pack_into("<H", payload, 4, PERIOD_FRAMES)
    struct.pack_into("<H", payload, 6, CHANNELS)
    struct.pack_into("<H", payload, 8, 1)  # S32LE
    struct.pack_into("<H", payload, 10, 1)  # 7.1.4 layout
    struct.pack_into("<I", payload, 12, 0)
    payload[16:48] = LAYOUT_HASH
    return frame(USB_CONFIG, bytes(payload), seq=seq)


def config_ack(seq):
    return frame(USB_ACK, struct.pack("<HH", USB_CONFIG, 0), seq=seq)


def parse_frame(raw):
    if len(raw) < USB_HEADER:
        raise AssertionError(f"short Aurora frame: {len(raw)}")
    magic, version, kind, flags, seq, pts, payload_len, aux = HEADER.unpack(
        raw[:USB_HEADER]
    )
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


def recv_kind(sock, wanted, timeout=1.0):
    deadline = time.monotonic() + timeout
    old_timeout = sock.gettimeout()
    try:
        while time.monotonic() < deadline:
            sock.settimeout(max(0.01, deadline - time.monotonic()))
            item = parse_frame(sock.recv(USB_HEADER + PCM_BYTES + 128))
            if item["kind"] == wanted:
                return item
        raise AssertionError(f"Aurora frame kind {wanted} not received")
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


def send_pcm(source, seq, sample):
    payload = struct.pack("<i", sample) * (PERIOD_FRAMES * CHANNELS)
    source.sendall(
        frame(
            USB_PCM,
            payload,
            seq=seq,
            aux=(CHANNELS << 16) | PERIOD_FRAMES,
        )
    )


def ramp_to_audible(source, bridge, seq_base, sample):
    peaks = []
    flags = []
    for i in range(16):
        send_pcm(source, seq_base + i, sample)
        out = recv_kind(bridge, USB_PCM)
        peaks.append(pcm_peak(out["payload"]))
        flags.append(out["flags"])
    assert any(p > 0 for p in peaks), "granted source never became audible"
    assert peaks[-1] >= int(sample * 0.95), peaks[-4:]
    assert any(f & USB_FLAG_DISCONTINUITY for f in flags), \
        "first post-GRANT PCM did not carry discontinuity"


def main():
    if len(sys.argv) != 3:
        raise SystemExit(
            "usage: test_source_gate.py /path/to/aurora-source-manager "
            "/path/to/aurora-source-gate"
        )
    manager_bin, gate_bin = sys.argv[1:]

    with tempfile.TemporaryDirectory(prefix="aurora-source-gate-") as td:
        manager_path = os.path.join(td, "manager.sock")
        hdmi_path = os.path.join(td, "hdmi.sock")
        local_path = os.path.join(td, "local.sock")
        bridge_path = os.path.join(td, "bridge.sock")

        bridge_listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        bridge_listener.bind(bridge_path)
        bridge_listener.listen(1)
        bridge_listener.settimeout(2)

        manager_env = os.environ.copy()
        manager_env["AURORA_SOURCE_MANAGER_SOCKET"] = manager_path
        manager = subprocess.Popen(
            [manager_bin],
            env=manager_env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )

        gate_env = os.environ.copy()
        gate_env["AURORA_SOURCE_MANAGER_SOCKET"] = manager_path
        gate_env["AURORA_HDMI_SOURCE_SOCKET"] = hdmi_path
        gate_env["AURORA_LOCAL_SOURCE_SOCKET"] = local_path
        gate_env["AURORA_USB_BRIDGE_SOCKET_REAL"] = bridge_path
        gate = subprocess.Popen(
            [gate_bin],
            env=gate_env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )

        hdmi = None
        local = None
        bridge = None
        try:
            bridge, _ = bridge_listener.accept()
            bridge.settimeout(1)
            hdmi = wait_connect(hdmi_path)
            hdmi.settimeout(1)
            local = wait_connect(local_path)
            local.settimeout(1)

            # Both adapters may prepare CONFIG, but the final mux must not let
            # either touch MCU state before source-manager ownership is granted.
            hdmi.sendall(config_frame(1))
            local.sendall(config_frame(2))
            expect_no_packet(bridge)

            sample = 0x20000000

            # First local PCM establishes local-source presence but is dropped.
            # The manager then grants local and the gate releases only its cached
            # CONFIG to the single MCU backend.
            send_pcm(local, 3, sample)
            local_config = recv_kind(bridge, USB_CONFIG, timeout=1.0)
            assert local_config["seq"] == 2
            expect_no_packet(bridge, timeout=0.03)

            # No PCM may pass until MCU CONFIG ACK is observed.
            send_pcm(local, 4, sample)
            expect_no_packet(bridge)
            bridge.sendall(config_ack(100))
            ack = recv_kind(local, USB_ACK)
            assert struct.unpack("<HH", ack["payload"]) == (USB_CONFIG, 0)
            ramp_to_audible(local, bridge, 10, sample)

            # HDMI encoded media is delivered to the HDMI decoder regardless of
            # current ownership, allowing decode warm-up while Local fades out.
            encoded_payload = b"\x72\xf8\x1f\x4e\x15\x00\x00\x00"
            bridge.sendall(frame(USB_ENCODED, encoded_payload, seq=200, pts=100))
            encoded = recv_kind(hdmi, USB_ENCODED)
            assert encoded["payload"] == encoded_payload

            # HDMI has higher priority. With no more Local PCM arriving, the
            # quiesce deadline completes the fade handoff and only then can HDMI
            # CONFIG reach the MCU backend.
            hdmi_config = recv_kind(bridge, USB_CONFIG, timeout=1.0)
            assert hdmi_config["seq"] == 1
            bridge.sendall(config_ack(201))
            ack = recv_kind(hdmi, USB_ACK)
            assert struct.unpack("<HH", ack["payload"]) == (USB_CONFIG, 0)
            ramp_to_audible(hdmi, bridge, 220, sample)

            # Local PCM remains present as a candidate but cannot overwrite the
            # active HDMI timeline.
            send_pcm(local, 300, sample)
            expect_no_packet(bridge)

            # Stop HDMI media long enough for idle detection. Local PCM sent
            # afterwards re-establishes local presence. Manager must revoke HDMI,
            # quiesce it, grant Local, and release Local's cached CONFIG again.
            time.sleep(1.10)
            send_pcm(local, 301, sample)
            local_config = recv_kind(bridge, USB_CONFIG, timeout=1.0)
            assert local_config["seq"] == 2
            bridge.sendall(config_ack(302))
            ack = recv_kind(local, USB_ACK)
            assert struct.unpack("<HH", ack["payload"]) == (USB_CONFIG, 0)
            ramp_to_audible(local, bridge, 320, sample)

            # Manager loss is a hard fail-closed boundary for both sources.
            terminate(manager)
            manager = None
            time.sleep(0.10)
            send_pcm(local, 400, sample)
            send_pcm(hdmi, 401, sample)
            expect_no_packet(bridge)

            # A physical USB-session reset invalidates all CONFIG and closes both
            # data adapters. They must reconnect through a fresh session.
            bridge.close()
            bridge = None
            hdmi.settimeout(1.0)
            local.settimeout(1.0)
            assert hdmi.recv(1) == b"", "HDMI source survived USB reset"
            assert local.recv(1) == b"", "Local source survived USB reset"

        finally:
            if hdmi is not None:
                hdmi.close()
            if local is not None:
                local.close()
            if bridge is not None:
                bridge.close()
            bridge_listener.close()
            terminate(gate)
            terminate(manager)

            gate_stderr = gate.stderr.read() if gate.stderr else ""
            if gate.returncode not in (0, -15):
                raise AssertionError(
                    f"source gate exited {gate.returncode}: {gate_stderr}"
                )

    print(
        "source-gate exclusive Local/HDMI ownership + CONFIG ACK + "
        "switch/reset/fail-closed tests passed"
    )


if __name__ == "__main__":
    main()
