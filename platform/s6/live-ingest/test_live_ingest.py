#!/usr/bin/env python3
import os
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

MAGIC = b"AUR0"
VERSION = 1
HEADER = struct.Struct("<4sHHIIQII")
HEADER_LEN = HEADER.size

KIND_ENCODED_IEC61937 = 1
KIND_PCM_S32LE = 2
KIND_CONFIG = 4
KIND_ACK = 5
FLAG_PTS_VALID = 1 << 0
FLAG_DISCONTINUITY = 1 << 1
FLAG_XRUN_RECOVERY = 1 << 3

PERIOD_FRAMES = 40
CHANNELS = 12
EXPECTED_LAYOUT_HASH = bytes.fromhex(
    "05063560d6c5c1b7d3709656cd8c644a6d2b52f5e81383771f26323442d0a244"
)


def frame(kind, payload=b"", flags=0, sequence=0, pts=0, aux=0):
    return HEADER.pack(
        MAGIC,
        VERSION,
        kind,
        flags,
        sequence,
        pts,
        len(payload),
        aux,
    ) + payload


def parse(packet):
    if len(packet) < HEADER_LEN:
        raise AssertionError(f"short packet: {len(packet)}")
    magic, version, kind, flags, sequence, pts, payload_len, aux = HEADER.unpack_from(packet)
    if magic != MAGIC:
        raise AssertionError(f"bad magic: {magic!r}")
    if version != VERSION:
        raise AssertionError(f"bad version: {version}")
    if len(packet) != HEADER_LEN + payload_len:
        raise AssertionError(
            f"packet length {len(packet)} != header+payload {HEADER_LEN + payload_len}"
        )
    return {
        "kind": kind,
        "flags": flags,
        "sequence": sequence,
        "pts": pts,
        "aux": aux,
        "payload": packet[HEADER_LEN:],
    }


def validate_config(config):
    assert config["kind"] == KIND_CONFIG, config
    assert len(config["payload"]) == 48
    cfg = config["payload"]
    assert struct.unpack_from("<I", cfg, 0)[0] == 48000
    assert struct.unpack_from("<H", cfg, 4)[0] == PERIOD_FRAMES
    assert struct.unpack_from("<H", cfg, 6)[0] == CHANNELS
    assert struct.unpack_from("<H", cfg, 8)[0] == 1
    assert struct.unpack_from("<H", cfg, 10)[0] == 1
    assert cfg[12:16] == b"\x00" * 4
    assert cfg[16:48] == EXPECTED_LAYOUT_HASH


def validate_pcm(pcm, pts, required_flags=0):
    assert pcm["kind"] == KIND_PCM_S32LE, pcm
    assert pcm["flags"] & FLAG_PTS_VALID
    assert (pcm["flags"] & required_flags) == required_flags, pcm
    assert pcm["pts"] == pts
    assert (pcm["aux"] >> 16) == CHANNELS
    assert (pcm["aux"] & 0xFFFF) == PERIOD_FRAMES
    assert len(pcm["payload"]) == CHANNELS * PERIOD_FRAMES * 4

    # 0.25f is converted with llround(x * 2147483647.0) -> 536870912.
    samples = struct.unpack(
        "<" + "i" * (CHANNELS * PERIOD_FRAMES), pcm["payload"]
    )
    assert samples[0] == 536_870_912
    assert samples[-1] == 536_870_912
    assert all(v == 536_870_912 for v in samples)


def make_ack(sequence=0):
    return frame(KIND_ACK, struct.pack("<HH", KIND_CONFIG, 0), sequence=sequence)


def make_encoded(encoded, pts, sequence=1, discontinuity=False):
    flags = FLAG_PTS_VALID
    if discontinuity:
        flags |= FLAG_DISCONTINUITY
    return frame(
        KIND_ENCODED_IEC61937,
        encoded,
        flags=flags,
        sequence=sequence,
        pts=pts,
    )


def launch_broker(broker, mock_orender, socket_path, encoded):
    env = os.environ.copy()
    env["AURORA_USB_BRIDGE_SOCKET"] = str(socket_path)
    env["AURORA_ORENDER_BIN"] = str(mock_orender)
    # This regression test isolates the broker/USB contract. The real
    # postprocessor has its own Rust tests and a separate integration test.
    env["AURORA_POSTPROCESS_BIN"] = "disabled"
    env["AURORA_MOCK_EXPECT_HEX"] = encoded.hex()
    env["AURORA_MOCK_SAMPLE"] = "0.25"
    return subprocess.Popen(
        [str(broker)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def assert_no_packet(conn, message, delay=0.20):
    time.sleep(delay)
    ready, _, _ = select.select([conn], [], [], 0)
    assert not ready, message


def stop_broker(proc):
    if proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
        try:
            proc.wait(timeout=3.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3.0)

    if proc.returncode not in (0, -signal.SIGTERM):
        stderr = proc.stderr.read() if proc.stderr else ""
        raise AssertionError(f"broker exit={proc.returncode}\n{stderr}")


def run_startup_case(broker, mock_orender, encoded, ack_before_encoded):
    with tempfile.TemporaryDirectory(prefix="aurora-live-ingest-") as td:
        socket_path = Path(td) / "usb-bridge.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(socket_path))
        listener.listen(1)
        listener.settimeout(5.0)
        proc = launch_broker(broker, mock_orender, socket_path, encoded)

        conn = None
        try:
            conn, _ = listener.accept()
            conn.settimeout(5.0)
            validate_config(parse(conn.recv(65536)))

            pts = 48_000
            if ack_before_encoded:
                conn.sendall(make_ack())
                conn.sendall(make_encoded(encoded, pts))
            else:
                # Encoded Atmos can arrive while STM32 is still validating
                # CONFIG. Rendered PCM may become ready, but it must neither
                # leak before ACK nor be discarded.
                conn.sendall(make_encoded(encoded, pts))
                assert_no_packet(conn, "PCM leaked before STM32 CONFIG ACK")
                conn.sendall(make_ack())

            validate_pcm(parse(conn.recv(65536)), pts)
        finally:
            if conn is not None:
                conn.close()
            listener.close()
            stop_broker(proc)


def run_discontinuity_case(broker, mock_orender, encoded):
    with tempfile.TemporaryDirectory(prefix="aurora-live-discontinuity-") as td:
        socket_path = Path(td) / "usb-bridge.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(socket_path))
        listener.listen(1)
        listener.settimeout(5.0)
        proc = launch_broker(broker, mock_orender, socket_path, encoded)

        conn = None
        try:
            conn, _ = listener.accept()
            conn.settimeout(5.0)
            validate_config(parse(conn.recv(65536)))
            conn.sendall(make_ack())

            first_pts = 48_000
            conn.sendall(make_encoded(encoded, first_pts, sequence=1))
            validate_pcm(parse(conn.recv(65536)), first_pts)

            # A source/segment discontinuity must restart renderer state and
            # anchor the new rendered stream to the new STM32 PTS. The same
            # encoded bytes are intentional: the mock validates that the new
            # child receives the first post-reset bytes, rather than stale data.
            second_pts = 144_000
            conn.sendall(
                make_encoded(
                    encoded,
                    second_pts,
                    sequence=2,
                    discontinuity=True,
                )
            )
            validate_pcm(
                parse(conn.recv(65536)),
                second_pts,
                required_flags=FLAG_DISCONTINUITY,
            )
        finally:
            if conn is not None:
                conn.close()
            listener.close()
            stop_broker(proc)


def run_reconnect_case(broker, mock_orender, encoded):
    with tempfile.TemporaryDirectory(prefix="aurora-live-reconnect-") as td:
        socket_path = Path(td) / "usb-bridge.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(socket_path))
        listener.listen(2)
        listener.settimeout(5.0)
        proc = launch_broker(broker, mock_orender, socket_path, encoded)

        first = None
        second = None
        try:
            first, _ = listener.accept()
            first.settimeout(5.0)
            validate_config(parse(first.recv(65536)))
            first.sendall(make_ack())
            first_pts = 48_000
            first.sendall(make_encoded(encoded, first_pts, sequence=1))
            validate_pcm(parse(first.recv(65536)), first_pts)

            # Simulate USB FunctionFS bridge loss. The broker must throw away
            # renderer/PTS/pending state, reconnect, and require a fresh CONFIG
            # handshake before any new PCM can reach STM32.
            first.close()
            first = None

            second, _ = listener.accept()
            second.settimeout(5.0)
            validate_config(parse(second.recv(65536)))
            assert_no_packet(second, "stale PCM leaked after USB bridge reconnect")

            second.sendall(make_ack(sequence=0))
            second_pts = 240_000
            second.sendall(make_encoded(encoded, second_pts, sequence=1))
            validate_pcm(
                parse(second.recv(65536)),
                second_pts,
                required_flags=FLAG_DISCONTINUITY | FLAG_XRUN_RECOVERY,
            )
        finally:
            if first is not None:
                first.close()
            if second is not None:
                second.close()
            listener.close()
            stop_broker(proc)


def main():
    if len(sys.argv) != 3:
        print("usage: test_live_ingest.py BROKER MOCK_ORENDER", file=sys.stderr)
        return 2

    broker = Path(sys.argv[1]).resolve()
    mock_orender = Path(sys.argv[2]).resolve()
    if not broker.exists():
        raise SystemExit(f"broker not found: {broker}")
    if not mock_orender.exists():
        raise SystemExit(f"mock renderer not found: {mock_orender}")

    mock_orender.chmod(mock_orender.stat().st_mode | 0o111)

    # Canonical S16_LE IEC61937 words. The test intentionally includes a DD+
    # type-0x15 preamble. Actual IEC61937/JOC parsing is owned by
    # Omniphony/Harletty; the mock verifies transport byte preservation.
    encoded = bytes.fromhex("72f81f4e15000600770b34127856")

    run_startup_case(broker, mock_orender, encoded, ack_before_encoded=True)
    run_startup_case(broker, mock_orender, encoded, ack_before_encoded=False)
    run_discontinuity_case(broker, mock_orender, encoded)
    run_reconnect_case(broker, mock_orender, encoded)

    print("Aurora live-ingest 40-frame startup/discontinuity/reconnect mock tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
