#!/usr/bin/env python3
import os
import signal
import socket
import struct
import subprocess
import sys
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

SOCKET_PATH = Path("/run/aurora/usb-bridge.sock")
EXPECTED_LAYOUT_HASH = bytes.fromhex(
    "40fb5d12fd76675aefb0344a8897145f0723981d20e205213b440e8bd3e127c0"
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
    SOCKET_PATH.parent.mkdir(parents=True, exist_ok=True)
    try:
        SOCKET_PATH.unlink()
    except FileNotFoundError:
        pass

    listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
    listener.bind(str(SOCKET_PATH))
    listener.listen(1)
    listener.settimeout(5.0)

    # Canonical S16_LE IEC61937 words. The test intentionally includes a DD+
    # type-0x15 preamble, but the mock renderer validates byte preservation only;
    # actual IEC61937/JOC parsing remains owned by Omniphony/Harletty.
    encoded = bytes.fromhex("72f81f4e15000600770b34127856")

    env = os.environ.copy()
    env["AURORA_ORENDER_BIN"] = str(mock_orender)
    env["AURORA_MOCK_EXPECT_HEX"] = encoded.hex()
    env["AURORA_MOCK_SAMPLE"] = "0.25"

    proc = subprocess.Popen(
        [str(broker)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    conn = None
    try:
        conn, _ = listener.accept()
        conn.settimeout(5.0)

        config = parse(conn.recv(65536))
        assert config["kind"] == KIND_CONFIG, config
        assert len(config["payload"]) == 48
        cfg = config["payload"]
        assert struct.unpack_from("<I", cfg, 0)[0] == 48000
        assert struct.unpack_from("<H", cfg, 4)[0] == 256
        assert struct.unpack_from("<H", cfg, 6)[0] == 12
        assert struct.unpack_from("<H", cfg, 8)[0] == 1
        assert struct.unpack_from("<H", cfg, 10)[0] == 1
        assert cfg[12:16] == b"\x00" * 4
        assert cfg[16:48] == EXPECTED_LAYOUT_HASH

        ack_payload = struct.pack("<HH", KIND_CONFIG, 0)
        conn.sendall(frame(KIND_ACK, ack_payload, sequence=0))

        pts = 48_000
        conn.sendall(
            frame(
                KIND_ENCODED_IEC61937,
                encoded,
                flags=FLAG_PTS_VALID,
                sequence=1,
                pts=pts,
            )
        )

        pcm = parse(conn.recv(65536))
        assert pcm["kind"] == KIND_PCM_S32LE, pcm
        assert pcm["flags"] & FLAG_PTS_VALID
        assert pcm["pts"] == pts
        assert (pcm["aux"] >> 16) == 12
        assert (pcm["aux"] & 0xFFFF) == 256
        assert len(pcm["payload"]) == 12 * 256 * 4

        # 0.25f is converted with llround(x * 2147483647.0) -> 536870912.
        samples = struct.unpack("<" + "i" * (12 * 256), pcm["payload"])
        assert samples[0] == 536_870_912
        assert samples[-1] == 536_870_912
        assert all(v == 536_870_912 for v in samples)

        print("Aurora live-ingest broker end-to-end mock test passed")
        return 0
    finally:
        if conn is not None:
            conn.close()
        listener.close()
        try:
            SOCKET_PATH.unlink()
        except FileNotFoundError:
            pass

        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=3.0)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3.0)

        if proc.returncode not in (0, -signal.SIGTERM):
            stderr = proc.stderr.read() if proc.stderr else ""
            print(f"broker exit={proc.returncode}\n{stderr}", file=sys.stderr)


if __name__ == "__main__":
    raise SystemExit(main())
