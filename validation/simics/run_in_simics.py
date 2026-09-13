"""Executable acceptance suite. Invoke through run-simics.ps1."""
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import traceback
import uuid

import conf
import simics

ROOT = Path(os.environ['AURORA_SIMICS_ROOT'])
sys.path.insert(0, str(ROOT / 'validation/simics'))
from aurora_audio_device import (CTRL, STATUS, SOURCE, LENGTH, RATE, CHANNELS,
                                 PERIOD, SUBMIT, DONE, IRQ, ERROR, FAULT, RAM_START,
                                 TDM_LANES, TDM_SLOTS_PER_LANE, FSYNC_HZ,
                                 BCLK_HZ, MCLK_HZ)
from mchstreamer_contract import self_test as mchstreamer_self_test

OUT = Path(os.environ['AURORA_SIMICS_OUT'])
BASE = 0x40000000
BLOCK = 240 * 48
results = []


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def main():
    obj = simics.SIM_create_object('aurora_audio_dma', 'aurora_audio',
                                 [['queue', conf.timer], ['target_mem', conf.phys_mem]])
    conf.phys_mem.map = list(conf.phys_mem.map) + [[BASE, obj.bank.regs, 0, 0, 0x100]]
    dev = obj.object_data
    mem = conf.phys_mem.iface.memory_space

    def wr(reg, value):
        mem.write(None, BASE + reg, tuple(struct.pack('<I', value)), 0)

    def rd(reg):
        return struct.unpack('<I', bytes(mem.read(None, BASE + reg, 4, 0)))[0]

    def advance(cycles=100000):
        simics.SIM_continue(cycles)

    def submit(data):
        mem.write(None, RAM_START, tuple(data), 0)
        wr(SUBMIT, 1)

    marker = struct.pack('<12f', *range(1, 13)) * 240

    # Real MMIO and DMA: no copy or completion before the scheduled deadline.
    submit(marker)
    wr(CTRL, 1)
    advance(99999)
    check(rd(DONE) == 0 and dev.frames == 0, 'early DMA completion')
    advance(1)
    check(rd(DONE) == 1 and rd(IRQ) == 1, 'period completion/IRQ missing')
    check(dev.last_tdm == (struct.pack('<12f', *range(1, 13)) + bytes(16)) * 240,
          'TDM slot identity/padding mismatch')
    lane0_expected = struct.pack('<8f', *range(1, 9)) * 240
    lane1_expected = (struct.pack('<4f', *range(9, 13)) + bytes(16)) * 240
    check(dev.last_tdm_lanes == (lane0_expected, lane1_expected),
          'dual-TDM8 lane mapping mismatch')
    wr(IRQ, 1)
    check(rd(IRQ) == 0, 'W1C IRQ failed')
    wr(CTRL, 0)
    advance()
    check(rd(DONE) == 1, 'stop left scheduled DMA active')
    results.append({'profile': 'deadline-mmio-irq-tdm-stop', 'verdict': 'pass'})

    profiles = {
        'unsupported-rate': (lambda: (wr(RATE, 44100), wr(CTRL, 1)), 1),
        'unsupported-channels': (lambda: (wr(CHANNELS, 16), wr(CTRL, 1)), 1),
        'invalid-period': (lambda: (wr(PERIOD, 0), wr(CTRL, 1)), 1),
        'short-descriptor': (lambda: (wr(LENGTH, BLOCK - 4), wr(SUBMIT, 1)), 3),
        'unaligned-dma': (lambda: (wr(SOURCE, RAM_START + 1), wr(SUBMIT, 1)), 4),
        'dma-out-of-bounds': (lambda: (wr(SOURCE, 0x1ffffffc), wr(SUBMIT, 1)), 4),
        'fifo-overrun': (lambda: [wr(SUBMIT, 1) for _ in range(3)], 5),
        'live-rate-change': (lambda: (wr(CTRL, 1), wr(RATE, 44100)), 6),
        'read-only-write': (lambda: wr(DONE, 99), 7),
        'underrun': (lambda: (wr(CTRL, 1), advance()), 8),
        'non-finite': (lambda: (submit(struct.pack('<f', float('nan')) + marker[4:]),
                                wr(CTRL, 1), advance()), 10),
    }
    for name, (inject, code) in profiles.items():
        wr(CTRL, 2)
        inject()
        check(rd(STATUS) == FAULT and rd(ERROR) == code, name + ': missing expected fault')
        completed = rd(DONE)
        wr(CTRL, 1)
        advance(200000)
        check(rd(STATUS) == FAULT and rd(DONE) == completed and not any(dev.last_tdm),
              name + ': did not remain muted and latched')
        check(rd(IRQ) & 2, name + ': missing fault IRQ')
        results.append({'profile': name, 'verdict': 'pass', 'expected_fault': code})

    for name, offset, width in [('unaligned-mmio', 1, 4), ('invalid-width', 0, 2),
                                ('unknown-register', 0x80, 4)]:
        wr(CTRL, 2)
        rejected = False
        try:
            mem.write(None, BASE + offset, tuple(bytes(width)), 0)
        except simics.SimExc_Memory:
            rejected = True
        check(rejected and rd(STATUS) == FAULT, name + ': transaction accepted')
        results.append({'profile': name, 'verdict': 'pass'})

    # Reset cancels an outstanding event, including one with queued data.
    wr(CTRL, 2)
    submit(marker)
    wr(CTRL, 1)
    advance(50000)
    wr(CTRL, 2)
    advance(100000)
    check(rd(DONE) == 0 and rd(STATUS) == 0, 'reset left stale event')
    results.append({'profile': 'reset-cancels-event', 'verdict': 'pass'})

    # Two distinct descriptors must retain FIFO ordering; DMA observes RAM at
    # the event, not a hidden copy made when the doorbell is written.
    wr(CTRL, 2)
    submit(bytes(BLOCK))
    from speaker_load import SpeakerBank
    probe_speakers = SpeakerBank()
    dev.output_sink = probe_speakers
    second = struct.pack('<12f', *[v / 100 for v in range(21, 33)]) * 240
    mem.write(None, RAM_START + BLOCK, tuple(second), 0)
    wr(SOURCE, RAM_START + BLOCK)
    wr(SUBMIT, 1)
    safe_marker = struct.pack('<12f', *[v / 100 for v in range(1, 13)]) * 240
    mem.write(None, RAM_START, tuple(safe_marker), 0)
    wr(CTRL, 1)
    advance()
    check(dev.pcm_hash.hexdigest() == hashlib.sha256(safe_marker).hexdigest(),
          'DMA was copied before its event or FIFO order changed')
    advance()
    check(rd(DONE) == 2 and dev.pcm_hash.hexdigest() == hashlib.sha256(safe_marker + second).hexdigest(),
          'second descriptor lost or reordered')
    check(all(probe_speakers.last_volts), 'speaker outputs not active before starvation')
    # Starvation after active output must mute, latch and stop completions.
    advance()
    check(rd(ERROR) == 8 and not any(dev.last_tdm), 'active stream underrun not muted')
    check(not any(probe_speakers.last_volts), 'DMA fault did not mute connected speaker loads')
    dev.output_sink = None
    results.append({'profile': 'fifo-order-deferred-dma-active-underrun', 'verdict': 'pass'})

    wr(CTRL, 2)
    submit(marker)
    wr(CTRL, 1)
    saved_map = list(conf.phys_mem.map)
    conf.phys_mem.map = [entry for entry in saved_map if entry[0] != RAM_START]
    try:
        advance()
        check(rd(ERROR) == 9 and rd(DONE) == 0 and not any(dev.last_tdm),
              'unmapped RAM DMA did not fail closed')
    finally:
        conf.phys_mem.map = saved_map
    results.append({'profile': 'dma-memory-disconnect', 'verdict': 'pass'})

    transport = mchstreamer_self_test()
    check(transport['verdict'] == 'pass', 'MCHStreamer/UAC2 contract failed')
    results.append({'profile': 'mchstreamer-uac2-dual-tdm8-contract',
                    'verdict': 'pass', **transport})

    # Replay the complete existing Aurora SpeakerPostProcessor output.
    pcm_path = Path(os.environ['AURORA_SIMICS_PCM'])
    expected_hash = hashlib.sha256()
    tdm_hash = hashlib.sha256()
    lane_hashes = [hashlib.sha256(), hashlib.sha256()]
    size = pcm_path.stat().st_size
    check(size > 0 and size % BLOCK == 0, 'PCM must contain whole 240-frame periods')
    wr(CTRL, 2)
    start = simics.SIM_time(conf.timer)
    from speaker_load import SpeakerBank, self_test as speaker_self_test
    speaker_tests = speaker_self_test()
    speakers = SpeakerBank()
    dev.output_sink = speakers
    with pcm_path.open('rb') as stream:
        for index in range(size // BLOCK):
            block = stream.read(BLOCK)
            expected_hash.update(block)
            # Independent construction of the real TDM16 firmware shape:
            # two synchronous TDM8 data lanes, eight 32-bit slots per lane.
            slots = bytearray(240 * 64)
            lane0 = bytearray(240 * 32)
            lane1 = bytearray(240 * 32)
            for frame in range(240):
                src = frame * 48
                l0 = frame * 32
                lane0[l0:l0 + 32] = block[src:src + 32]
                lane1[l0:l0 + 16] = block[src + 32:src + 48]
                slots[frame * 64:frame * 64 + 32] = lane0[l0:l0 + 32]
                slots[frame * 64 + 32:frame * 64 + 64] = lane1[l0:l0 + 32]
            lane_hashes[0].update(lane0)
            lane_hashes[1].update(lane1)
            tdm_hash.update(slots)
            submit(block)
            if index == 0:
                wr(CTRL, 1)
            advance()
            check(rd(DONE) == index + 1 and rd(STATUS) == 1, 'replay DMA accounting')
            check(speakers.error is None, 'speaker model rejected DMA output: ' + str(speakers.error))
            wr(IRQ, 1)
    elapsed = simics.SIM_time(conf.timer) - start
    wr(CTRL, 0)
    check(dev.frames == size // 48, 'frame count mismatch')
    check(dev.pcm_hash.hexdigest() == expected_hash.hexdigest(), 'DMA PCM hash mismatch')
    check(dev.tdm_hash.hexdigest() == tdm_hash.hexdigest(), 'TDM hash mismatch')
    check(dev.tdm_lane_hashes[0].hexdigest() == lane_hashes[0].hexdigest(),
          'TDM lane 0 hash mismatch')
    check(dev.tdm_lane_hashes[1].hexdigest() == lane_hashes[1].hexdigest(),
          'TDM lane 1 hash mismatch')
    error = max(abs(t - start - (i + 1) * .005) for i, t in enumerate(dev.times))
    check(error < 1e-7 and abs(elapsed - dev.frames / 48000) < 1e-7,
          'simulation timing drift exceeds two vacuum clock cycles')
    results.append({'profile': 'full-dsp-pcm-replay', 'verdict': 'pass',
                    'pcm_path': str(pcm_path), 'frames': dev.frames,
                    'periods': rd(DONE), 'simulated_seconds': elapsed,
                    'max_deadline_error_seconds': error, 'xruns': rd(ERROR),
                    'pcm_sha256': expected_hash.hexdigest(),
                    'tdm16_sha256': tdm_hash.hexdigest(),
                    'tdm_lane0_sha256': lane_hashes[0].hexdigest(),
                    'tdm_lane1_sha256': lane_hashes[1].hexdigest(),
                    'tdm_lanes': TDM_LANES,
                    'slots_per_lane': TDM_SLOTS_PER_LANE,
                    'fsync_hz': FSYNC_HZ, 'bclk_hz': BCLK_HZ,
                    'mclk_hz': MCLK_HZ})
    check(speakers.frames == dev.frames and all(v > 0 for v in speakers.energy),
          'speaker frame/activity mismatch')
    results.append({'profile': 'dac-amplifier-speaker-loads', 'verdict': 'pass',
                    'self_tests': speaker_tests, **speakers.report()})


try:
    main()
    report = {'verdict': 'pass', 'evidence_class': 'simics-device-model',
              'run_id': os.environ.get('AURORA_SIMICS_RUN_ID', str(uuid.uuid4())),
              'source_sha256': {name: hashlib.sha256((ROOT / 'validation/simics' / name).read_bytes()).hexdigest()
                                for name in ('aurora_audio_device.py', 'speaker_load.py', 'run_in_simics.py', 'aurora.simics', 'run-simics.ps1')},
              'truth_boundary': 'Abstract Aurora DMA contract on vacuum; no guest OS, vendor hardware, live decoder or physical timing proof.',
              'profiles': results}
    OUT.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print('AURORA-SIMICS-HARNESS-PASS')
    simics.SIM_quit(0)
except Exception:
    traceback.print_exc()
    OUT.write_text(json.dumps({'verdict': 'fail', 'profiles': results,
                               'error': traceback.format_exc()}, indent=2), encoding='utf-8')
    simics.SIM_quit(1)
