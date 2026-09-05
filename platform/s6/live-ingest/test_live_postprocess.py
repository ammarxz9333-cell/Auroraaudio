#!/usr/bin/env python3
import os
import signal
import socket
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

MAGIC = b"AUR0"
VERSION = 1
HEADER = struct.Struct("<4sHHIIQII")
HEADER_LEN = HEADER.size

KIND_ENCODED_IEC61937 = 1
KIND_PCM_S32LE = 2
KIND_CLOCK_REPORT = 3
KIND_CONFIG = 4
KIND_ACK = 5
FLAG_PTS_VALID = 1 << 0
FLAG_DISCONTINUITY = 1 << 1

PERIOD_FRAMES = 40
CHANNELS = 12
PCM_BYTES = PERIOD_FRAMES * CHANNELS * 4


def frame(kind, payload=b"", flags=0, sequence=0, pts=0, aux=0):
    return HEADER.pack(MAGIC, VERSION, kind, flags, sequence, pts, len(payload), aux) + payload


def parse(packet):
    if len(packet) < HEADER_LEN:
        raise AssertionError(f"short packet: {len(packet)}")
    magic, version, kind, flags, sequence, pts, payload_len, aux = HEADER.unpack_from(packet)
    assert magic == MAGIC
    assert version == VERSION
    assert len(packet) == HEADER_LEN + payload_len
    return {
        "kind": kind,
        "flags": flags,
        "sequence": sequence,
        "pts": pts,
        "aux": aux,
        "payload": packet[HEADER_LEN:],
    }


def ack_config():
    return frame(KIND_ACK, struct.pack("<HH", KIND_CONFIG, 0))


def encoded_packet(encoded, pts, sequence, discontinuity=False):
    flags = FLAG_PTS_VALID | (FLAG_DISCONTINUITY if discontinuity else 0)
    return frame(KIND_ENCODED_IEC61937, encoded, flags=flags, sequence=sequence, pts=pts)


def clock_report(sequence, sink, source, queued):
    payload = struct.pack("<QQII", sink, source, queued, 0)
    return frame(KIND_CLOCK_REPORT, payload, sequence=sequence, pts=sink)


def validate_processed_pcm(packet, expected_pts, required_flags=0):
    parsed = parse(packet)
    assert parsed["kind"] == KIND_PCM_S32LE, parsed
    assert parsed["flags"] & FLAG_PTS_VALID
    assert (parsed["flags"] & required_flags) == required_flags, parsed
    assert parsed["pts"] == expected_pts, parsed
    assert (parsed["aux"] >> 16) == CHANNELS
    assert (parsed["aux"] & 0xFFFF) == PERIOD_FRAMES
    assert len(parsed["payload"]) == PCM_BYTES

    samples = struct.unpack("<" + "i" * (CHANNELS * PERIOD_FRAMES), parsed["payload"])
    peak = max(abs(sample) for sample in samples)
    # The postprocessor starts from a smoothed mute/gain state, so the first
    # period is intentionally not bit-identical to the renderer's 0.25 input.
    assert peak > 0, "postprocessor produced only silence"
    # -1 dBFS limiter ceiling, with one integer LSB of tolerance.
    limiter_ceiling = int(round((10 ** (-1.0 / 20.0)) * 2_147_483_647)) + 1
    assert peak <= limiter_ceiling, (peak, limiter_ceiling)
    return samples


def stop(proc):
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


def main():
    if len(sys.argv) != 4:
        print("usage: test_live_postprocess.py BROKER MOCK_ORENDER POSTPROCESS", file=sys.stderr)
        return 2

    broker = Path(sys.argv[1]).resolve()
    mock_orender = Path(sys.argv[2]).resolve()
    postprocess = Path(sys.argv[3]).resolve()
    for path in (broker, mock_orender, postprocess):
        if not path.exists():
            raise SystemExit(f"missing test executable: {path}")

    encoded = bytes.fromhex("72f81f4e15000600770b34127856")
    with tempfile.TemporaryDirectory(prefix="aurora-live-postprocess-") as td:
        socket_path = Path(td) / "usb-bridge.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(str(socket_path))
        listener.listen(1)
        listener.settimeout(5.0)

        env = os.environ.copy()
        env["AURORA_USB_BRIDGE_SOCKET"] = str(socket_path)
        env["AURORA_ORENDER_BIN"] = str(mock_orender)
        env["AURORA_POSTPROCESS_BIN"] = str(postprocess)
        env["AURORA_MOCK_EXPECT_HEX"] = encoded.hex()
        env["AURORA_MOCK_SAMPLE"] = "0.25"
        # Give the ASRC enough source quanta to cover its sinc startup history
        # and several 40-frame outputs without making the mock timing-sensitive.
        env["AURORA_MOCK_BLOCKS"] = "16"
        env["AURORA_DRIFT_TARGET_FRAMES"] = "120"
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
            assert config["kind"] == KIND_CONFIG
            conn.sendall(ack_config())

            # Feed live sink telemetry before audio so the real control pipe,
            # DriftController and Rubato ASRC path are exercised together.
            conn.sendall(clock_report(1, 48_000, 48_000, 120))
            first_pts = 48_000
            conn.sendall(encoded_packet(encoded, first_pts, sequence=2))
            first = validate_processed_pcm(conn.recv(65536), first_pts)
            assert any(sample != 536_870_912 for sample in first), (
                "postprocessor appears bypassed: output remained raw mock PCM"
            )

            # A real source discontinuity must rebuild both child processes and
            # the first post-reset PCM period must explicitly carry the flag.
            second_pts = 144_000
            conn.sendall(
                encoded_packet(encoded, second_pts, sequence=3, discontinuity=True)
            )
            # Packets already sent before the reset cannot be withdrawn from
            # this socket. Validate the bounded old epoch while draining it;
            # the first packet of the new epoch must still carry the flag.
            expected_old_pts = first_pts + PERIOD_FRAMES
            for _ in range(17):
                packet = conn.recv(65536)
                if parse(packet)["pts"] == second_pts:
                    validate_processed_pcm(packet, second_pts, FLAG_DISCONTINUITY)
                    break
                validate_processed_pcm(packet, expected_old_pts)
                expected_old_pts += PERIOD_FRAMES
                assert expected_old_pts <= first_pts + 16 * PERIOD_FRAMES
            else:
                raise AssertionError("no discontinuity-marked new epoch within mock output bound")
        finally:
            if conn is not None:
                conn.close()
            listener.close()
            stop(proc)

    print("Aurora live broker + postprocessor + drift-control integration passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
