#!/usr/bin/env python3
import os
import struct
import sys

expected = bytes.fromhex(os.environ.get("AURORA_MOCK_EXPECT_HEX", ""))
received = bytearray()
while len(received) < len(expected):
    chunk = sys.stdin.buffer.read(len(expected) - len(received))
    if not chunk:
        sys.exit(2)
    received += chunk

if bytes(received) != expected:
    print(
        f"mock_orender: stdin mismatch got={bytes(received).hex()} expected={expected.hex()}",
        file=sys.stderr,
    )
    sys.exit(3)

channels = 12
frames = 256
sample = float(os.environ.get("AURORA_MOCK_SAMPLE", "0.25"))
payload = struct.pack("<f", sample) * (channels * frames)
sys.stdout.buffer.write(payload)
sys.stdout.buffer.flush()

# Keep the process alive so the broker does not enter a renderer restart loop
# before the integration test has received and checked its PCM period.
while True:
    chunk = sys.stdin.buffer.read(4096)
    if not chunk:
        break
