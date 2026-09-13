"""Ideal 24-bit DAC, unity amplifier and resistive speaker-load model.

No acoustics, enclosure, crossover, impedance curve or vendor DAC is implied.
"""
import hashlib
import math
import struct

ROLES = ('FL', 'FR', 'C', 'LFE', 'BL', 'BR', 'SL', 'SR', 'TFL', 'TFR', 'TBL', 'TBR')
SCALE = 8388607


class SpeakerBank:
    def __init__(self):
        self.frames = 0
        self.energy = [0.] * 12
        self.peaks = [0.] * 12
        self.hashes = [hashlib.sha256() for _ in ROLES]
        self.last_volts = [0.] * 12
        self.error = None
        self.connected = [True] * 12

    def mute(self):
        self.last_volts = [0.] * 12

    def consume_lanes(self, lane0, lane1):
        if len(lane0) != len(lane1) or len(lane0) % 32:
            self.error = 'invalid-tdm-lane-size'
            self.mute()
            return
        combined = b''.join(lane0[i:i + 32] + lane1[i:i + 32]
                            for i in range(0, len(lane0), 32))
        self.consume(combined)

    def consume(self, tdm):
        if self.error:
            return
        if len(tdm) % 64:
            self.error = 'invalid-tdm-size'
        elif not all(self.connected):
            self.error = 'speaker-disconnected'
        else:
            rows = list(struct.iter_unpack('<16f', tdm))
            if any(not math.isfinite(v) or abs(v) > 1 for row in rows for v in row[:12]):
                self.error = 'non-finite-or-dac-overrange'
            elif any(any(row[12:]) for row in rows):
                self.error = 'nonzero-unused-slot'
        if self.error:
            self.last_volts = [0.] * 12
            return
        for row in rows:
            for channel, value in enumerate(row[:12]):
                q = round(value * SCALE)
                voltage = q / SCALE * 2.0
                self.hashes[channel].update(struct.pack('<i', q))
                self.energy[channel] += voltage * voltage
                self.peaks[channel] = max(self.peaks[channel], abs(voltage))
                self.last_volts[channel] = voltage
            self.frames += 1

    def report(self):
        return {'frames': self.frames, 'error': self.error,
                'assumptions': 'Ideal symmetric signed-24-bit quantization, unity amplifier, 2 V peak full scale, twelve 8-ohm resistive loads; not acoustic measurements.',
                'channels': [{'role': role, 'dac_sha256': self.hashes[i].hexdigest(),
                              'peak_volts': self.peaks[i],
                              'rms_volts': math.sqrt(self.energy[i] / self.frames) if self.frames else 0,
                              'mean_load_watts': self.energy[i] / self.frames / 8 if self.frames else 0}
                             for i, role in enumerate(ROLES)]}


def self_test():
    tests = []
    for channel in range(12):
        bank = SpeakerBank()
        values = [0.] * 16
        values[channel] = .5
        bank.consume(struct.pack('<16f', *values))
        assert [i for i, v in enumerate(bank.last_volts) if v] == [channel]
    tests.append('twelve-speaker-impulse-routing')
    for fault in ('disconnect', 'overrange', 'nan', 'padding', 'bad-length'):
        bank = SpeakerBank()
        bank.consume(struct.pack('<16f', *([.25] * 12 + [0.] * 4)))
        values = [.25] * 12 + [0.] * 4
        if fault == 'disconnect':
            bank.connected[4] = False
        elif fault == 'overrange':
            values[0] = 1.1
        elif fault == 'nan':
            values[0] = float('nan')
        elif fault == 'padding':
            values[15] = .1
        data = struct.pack('<16f', *values)
        bank.consume(data[:-1] if fault == 'bad-length' else data)
        assert bank.error and not any(bank.last_volts) and bank.frames == 1
        bank.consume(struct.pack('<16f', *([0.] * 16)))
        assert bank.frames == 1
        tests.append(fault)
    return tests
