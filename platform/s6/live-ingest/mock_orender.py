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
frames = 40
sample = float(os.environ.get("AURORA_MOCK_SAMPLE", "0.25"))
blocks = int(os.environ.get("AURORA_MOCK_BLOCKS", "1"))
if blocks <= 0 or blocks > 4096:
    print(f"mock_orender: invalid AURORA_MOCK_BLOCKS={blocks}", file=sys.stderr)
    sys.exit(4)
payload = struct.pack("<f", sample) * (channels * frames)
for _ in range(blocks):
    sys.stdout.buffer.write(payload)
sys.stdout.buffer.flush()

# Keep the process alive so the broker does not enter a renderer restart loop
# before the integration test has received and checked its PCM periods.
while True:
    chunk = sys.stdin.buffer.read(4096)
    if not chunk:
        break
