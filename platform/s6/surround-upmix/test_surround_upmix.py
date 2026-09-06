#!/usr/bin/env python3
"""Real FFmpeg E-AC-3/AC-3 decoding tests; fixtures contain no Atmos objects."""
import array
import math
import os
from pathlib import Path
import select
import subprocess
import sys
import threading
import time

FFMPEG = os.environ.get("AURORA_FFMPEG_BIN", "/usr/bin/ffmpeg")


def run(args, data=None):
    return subprocess.run(args, input=data, capture_output=True, check=True, timeout=20)


def encode(expressions, layout="5.1(side)", codec="eac3", duration=1):
    signal = "|".join(expressions)
    return run([
        FFMPEG, "-v", "error", "-f", "lavfi", "-i",
        f"aevalsrc={signal}:s=48000:d={duration}:c={layout}",
        "-c:a", codec, "-b:a", "640k", "-f", "spdif", "pipe:1",
    ]).stdout


def samples(data, channels):
    assert data and len(data) % (channels * 4) == 0, "truncated or empty PCM"
    values = array.array("f")
    values.frombytes(data)
    if sys.byteorder != "little":
        values.byteswap()
    assert all(math.isfinite(x) for x in values), "nonfinite PCM"
    return values


def decode_reference(data):
    return samples(run([
        FFMPEG, "-v", "error", "-f", "spdif", "-i", "pipe:0",
        "-ac", "8", "-ar", "48000", "-c:a", "pcm_f32le", "-f", "f32le", "pipe:1",
    ], data).stdout, 8)


def verify_bed_and_heights(script, encoded, expect_heights):
    decoded = run(["sh", str(script)], encoded)
    assert b"objects_decoded=false heights=synthetic" in decoded.stderr
    actual = samples(decoded.stdout, 12)
    reference = decode_reference(encoded)
    assert len(actual) // 12 == len(reference) // 8, "bed duration changed"
    error = max(abs(actual[f * 12 + c] - reference[f * 8 + c])
                for f in range(len(reference) // 8) for c in range(8))
    assert error < 1e-6, f"bed remapped/modified: max error {error}"
    heights = [max(abs(x) for x in actual[c::12]) for c in range(8, 12)]
    if expect_heights:
        assert all(x > 0.001 for x in heights), heights
    else:
        assert max(heights) < 1e-6, f"dialogue/mono/LFE leaked to heights: {heights}"
    return len(actual) // 12


def verify_before_eof(script, encoded):
    proc = subprocess.Popen(["sh", str(script)], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    failures = []

    def feed():
        try:
            proc.stdin.write(encoded)
            proc.stdin.flush()
            # Keep the stream open: decoder must output without waiting for EOF.
        except (BrokenPipeError, ValueError) as exc:
            failures.append(str(exc))

    writer = threading.Thread(target=feed, daemon=True)
    writer.start()
    try:
        ready, _, _ = select.select([proc.stdout], [], [], 5)
        assert ready, "no PCM before input EOF"
        block = os.read(proc.stdout.fileno(), 1920)
        assert block, "decoder exited without PCM"
        samples(block, 12)
    finally:
        proc.kill()
        proc.wait(timeout=5)
        writer.join(timeout=5)
        assert not writer.is_alive(), "input feeder did not stop"
        proc.stdin.close()
        proc.stdout.close()
        proc.stderr.close()


def main():
    script = Path(sys.argv[1] if len(sys.argv) > 1 else
                  Path(__file__).with_name("aurora-surround-upmix.sh")).resolve()
    tone = lambda f: f"0.1*sin(2*PI*{f}*t)"
    mixed = encode([tone(440), tone(550), tone(330), tone(50), tone(660), tone(770)])
    frames = verify_bed_and_heights(script, mixed, True)
    verify_bed_and_heights(script, encode(["0", "0", tone(440), "0", "0", "0"]), False)
    verify_bed_and_heights(script, encode(["0", "0", "0", tone(50), "0", "0"]), False)
    verify_bed_and_heights(script, encode([tone(440)] * 2, "stereo", "ac3"), False)
    verify_before_eof(script, mixed)
    invalid = subprocess.run(["sh", str(script)], input=b"not an audio carrier",
                             capture_output=True, timeout=10)
    assert invalid.returncode != 0 and not invalid.stdout
    # Throughput is local software evidence only, not S6 or physical latency.
    long_input = encode([tone(440), tone(550), "0", "0", tone(660), tone(770)], duration=10)
    started = time.monotonic()
    long_result = run(["sh", str(script)], long_input)
    elapsed = time.monotonic() - started
    duration = len(long_result.stdout) / (48000 * 12 * 4)
    print(f"PASS: real E-AC-3 IEC61937 -> 7.1.4 ({frames} frames); bed preserved; "
          "synthetic heights active; center/LFE/mono isolated; output before EOF; invalid input rejected")
    print(f"HOST ONLY: {duration:.3f}s audio processed in {elapsed:.3f}s "
          f"({duration / elapsed:.1f}x offline throughput); no JOC or hardware proof")


if __name__ == "__main__":
    main()
