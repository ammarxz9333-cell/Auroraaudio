"""Executable contract for MCHStreamer TDM16 + generic UAC2 async behavior.

This models documented transport semantics, not XMOS firmware internals or
physical USB/TDM signal integrity. It is executed from the Intel Simics suite.
"""
import math

FS_HZ = 48_000
USB_HS_MICROFRAMES_HZ = 8_000
LOGICAL_CHANNELS = 16
ACTIVE_CHANNELS = 12
TDM_LANES = 2
SLOTS_PER_LANE = 8
SLOT_BITS = 32
VALID_BITS = 24
FSYNC_HZ = FS_HZ
BCLK_HZ = FS_HZ * SLOTS_PER_LANE * SLOT_BITS
MCLK_HZ = 24_576_000
S24_MAX = (1 << 23) - 1


def _q24(value):
    if not math.isfinite(value) or abs(value) > 1.0:
        raise ValueError('invalid-dac-sample')
    q = round(value * S24_MAX)
    if q < 0:
        q += 1 << 24
    return q & 0xFFFFFF
def pack_wire_frame(samples12):
    if len(samples12) != ACTIVE_CHANNELS:
        raise ValueError('expected-12-channels')
    words = [_q24(v) << 8 for v in samples12] + [0] * 4
    lane0 = b''.join(w.to_bytes(4, 'big') for w in words[:8])
    lane1 = b''.join(w.to_bytes(4, 'big') for w in words[8:16])
    return lane0, lane1


def packetize_feedback(rate_hz, microframes):
    """Integer USB packet frame counts from a fractional async feedback rate."""
    phase = 0.0
    packets = []
    per_microframe = rate_hz / USB_HS_MICROFRAMES_HZ
    for _ in range(microframes):
        phase += per_microframe
        frames = int(phase + 1e-12)
        phase -= frames
        packets.append(frames)
    return packets


def simulate_async(*, ppm=0.0, seconds=2.0, feedback=True,
                   initial_frames=12.0, capacity_frames=48.0,
                   dropped_microframes=()):
    device_rate = FS_HZ * (1.0 + ppm / 1_000_000.0)
    feedback_rate = device_rate if feedback else FS_HZ
    count = int(seconds * USB_HS_MICROFRAMES_HZ)
    packets = packetize_feedback(feedback_rate, count)
    buffer_frames = initial_frames
    minimum = buffer_frames
    maximum = buffer_frames
    underrun = False
    overrun = False
    consumed_per_microframe = device_rate / USB_HS_MICROFRAMES_HZ
    dropped = set(dropped_microframes)
    for index, frames in enumerate(packets):
        if index in dropped:
            frames = 0
        buffer_frames += frames
        buffer_frames -= consumed_per_microframe
        minimum = min(minimum, buffer_frames)
        maximum = max(maximum, buffer_frames)
        if buffer_frames < -1e-9:
            underrun = True
            break
        if buffer_frames > capacity_frames + 1e-9:
            overrun = True
            break
    return {
        'device_rate_hz': device_rate,
        'feedback_rate_hz': feedback_rate,
        'microframes': count,
        'nominal_frames_per_microframe': FS_HZ / USB_HS_MICROFRAMES_HZ,
        'packet_sizes': sorted(set(packets)),
        'buffer_min_frames': minimum,
        'buffer_max_frames': maximum,
        'buffer_end_frames': buffer_frames,
        'underrun': underrun,
        'overrun': overrun,
    }


def self_test():
    assert FSYNC_HZ == 48_000
    assert BCLK_HZ == 12_288_000
    assert MCLK_HZ == 24_576_000
    assert FS_HZ / USB_HS_MICROFRAMES_HZ == 6
    samples = (-1.0, -0.75, -0.5, -0.25, 0.0, 0.125,
               0.25, 0.5, 0.75, 1.0, -0.125, 0.0625)
    lane0, lane1 = pack_wire_frame(samples)
    assert len(lane0) == len(lane1) == 32
    assert lane1[16:] == bytes(16)
    words = [int.from_bytes(lane0[i:i + 4], 'big') for i in range(0, 32, 4)]
    words += [int.from_bytes(lane1[i:i + 4], 'big') for i in range(0, 32, 4)]
    assert all((word & 0xFF) == 0 for word in words)
    assert [word >> 8 for word in words[:12]] == [_q24(v) for v in samples]
    assert words[12:] == [0, 0, 0, 0]

    fast = simulate_async(ppm=250.0, seconds=2.0, feedback=True)
    slow = simulate_async(ppm=-250.0, seconds=2.0, feedback=True)
    assert not fast['underrun'] and not fast['overrun']
    assert not slow['underrun'] and not slow['overrun']
    assert fast['packet_sizes'] == [6, 7]
    assert slow['packet_sizes'] == [5, 6]

    no_feedback_fast = simulate_async(ppm=250.0, seconds=2.0, feedback=False)
    no_feedback_slow = simulate_async(ppm=-250.0, seconds=4.0, feedback=False)
    assert no_feedback_fast['underrun']
    assert no_feedback_slow['overrun']
    dropped = simulate_async(ppm=0.0, seconds=.01, feedback=True,
                             dropped_microframes=(0, 1, 2))
    assert dropped['underrun']

    return {
        'verdict': 'pass',
        'usb_mode': 'UAC2 asynchronous contract',
        'usb_high_speed_microframe_hz': USB_HS_MICROFRAMES_HZ,
        'nominal_frames_per_microframe': 6,
        'tdm_lanes': TDM_LANES,
        'slots_per_lane': SLOTS_PER_LANE,
        'slot_bits': SLOT_BITS,
        'valid_bits': VALID_BITS,
        'active_channels': ACTIVE_CHANNELS,
        'logical_channels': LOGICAL_CHANNELS,
        'fsync_hz': FSYNC_HZ,
        'bclk_hz': BCLK_HZ,
        'mclk_hz': MCLK_HZ,
        'healthy_plus_250ppm': fast,
        'healthy_minus_250ppm': slow,
        'no_feedback_plus_250ppm': no_feedback_fast,
        'no_feedback_minus_250ppm': no_feedback_slow,
        'triple_microframe_drop': dropped,
        'wire_mapping': 'lane0=ch1-8; lane1=ch9-12 plus zero ch13-16',
        'truth_boundary': ('Documented transport contract only; no XMOS firmware, USB PHY, '
                           'signal-integrity or physical DAC timing proof.'),
    }
