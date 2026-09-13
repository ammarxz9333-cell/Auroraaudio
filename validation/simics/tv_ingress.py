"""Run inside Simics: abstract TV passthrough and eARC receiver DMA.

Input is an official Dolby fixture, never a simulated Netflix authentication.
This models the audio boundary, not eARC discovery signalling or TV firmware.
"""
import hashlib
import json
import os
from pathlib import Path
import struct
import traceback

import conf
import pyobj
import simics

BURST = 24576
ADDR = 0x10000000


class aurora_tv_audio(pyobj.ConfObject):
    """TV passthrough boundary with a single receiver DMA buffer."""
    _class_desc = 'Aurora TV audio source model'

    def _initialize(self):
        super()._initialize()
        self.event = simics.SIM_register_event('tv-audio-burst', None, simics.Sim_EC_Notsaved,
            lambda obj, arg: obj.object_data.tick(), None, None, None, None)
        self.reset()

    def reset(self):
        self.state = 'idle'
        self.ready = False
        self.sent = 0
        self.bursts = []
        self.times = []
        self.link = True
        self.error = None

    def fail(self, reason):
        self.state = 'fault'
        self.error = reason
        self.ready = False
        simics.SIM_event_cancel_time(conf.timer, self.event, self.obj, None, None)

    def start(self, bursts, *, passthrough=True, receiver_eac3=True):
        self.reset()
        if not passthrough or not receiver_eac3:
            self.fail('unsupported-audio-contract')
            return
        self.bursts = bursts
        self.state = 'streaming'
        self.post()

    def post(self):
        simics.SIM_event_post_time(conf.timer, self.event, self.obj, .032, None)

    def tick(self):
        if self.state != 'streaming':
            return
        if not self.link:
            self.fail('link-lost')
        elif self.ready:
            self.fail('receiver-overrun')
        elif self.sent < len(self.bursts):
            conf.phys_mem.iface.memory_space.write(self.obj, ADDR, tuple(self.bursts[self.sent]), 0)
            self.ready = True
            self.sent += 1
            self.times.append(simics.SIM_time(conf.timer))
            if self.sent < len(self.bursts):
                self.post()


def validate_burst(burst):
    if len(burst) != BURST:
        raise ValueError('burst-size')
    pa, pb, pc, pd = struct.unpack_from('<4H', burst)
    if (pa, pb) != (0xf872, 0x4e1f) or pc != 0x15:
        raise ValueError('IEC-header')
    if not 0 < pd <= BURST - 8 or pd % 2 or any(burst[8 + pd:]):
        raise ValueError('payload-length-or-padding')
    payload = bytearray(pd)
    payload[0::2], payload[1::2] = burst[9:8 + pd:2], burst[8:8 + pd:2]
    if payload[:2] != b'\x0b\x77':
        raise ValueError('EAC3-sync')
    return bytes(payload)


def main():
    out = Path(os.environ['AURORA_TV_OUT'])
    carrier = Path(os.environ['AURORA_TV_CARRIER']).read_bytes()
    assert len(carrier) == BURST * 2360
    bursts = [carrier[i:i + BURST] for i in range(0, len(carrier), BURST)]
    obj = simics.SIM_create_object('aurora_tv_audio', 'aurora_tv', [['queue', conf.timer]])
    tv = obj.object_data
    tests = []
    for name, kwargs in [('tv-pcm-mode', {'passthrough': False}),
                         ('receiver-no-eac3', {'receiver_eac3': False})]:
        tv.start(bursts, **kwargs)
        assert tv.state == 'fault' and tv.sent == 0
        tests.append({'profile': name, 'verdict': 'pass'})
    tv.start(bursts)
    tv.link = False
    simics.SIM_continue(640000)
    assert tv.error == 'link-lost' and tv.sent == 0
    tests.append({'profile': 'earc-link-loss', 'verdict': 'pass'})
    tv.start(bursts)
    simics.SIM_continue(1280001)
    assert tv.error == 'receiver-overrun' and tv.sent == 1
    tests.append({'profile': 'receiver-dma-overrun', 'verdict': 'pass'})
    for name, position in [('wrong-codec', 4), ('corrupted-padding', BURST - 1), ('sync-loss', 0)]:
        bad = bytearray(bursts[0])
        bad[position] ^= 1
        try:
            validate_burst(bad)
        except ValueError:
            tests.append({'profile': name, 'verdict': 'pass'})
        else:
            raise AssertionError(name + ' was accepted')

    tv.start(bursts)
    start = simics.SIM_time(conf.timer)
    extracted = hashlib.sha256()
    received = hashlib.sha256()
    with (out / 'received.spdif').open('wb') as stream:
        for index in range(len(bursts)):
            simics.SIM_continue(640000)
            assert tv.state == 'streaming' and tv.ready and tv.sent == index + 1
            data = b''.join(bytes(conf.phys_mem.iface.memory_space.read(obj, ADDR + offset,
                            min(1024, BURST - offset), 0)) for offset in range(0, BURST, 1024))
            extracted.update(validate_burst(data))
            received.update(data)
            stream.write(data)
            tv.ready = False
    assert received.hexdigest() == hashlib.sha256(carrier).hexdigest()
    assert extracted.hexdigest() == '0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0'
    error = max(abs(t - start - (i + 1) * .032) for i, t in enumerate(tv.times))
    assert error < 1e-7
    report = {'verdict': 'pass', 'source': 'official Dolby fixture; NOT Netflix',
              'bursts': tv.sent, 'simulated_seconds': simics.SIM_time(conf.timer) - start,
              'max_deadline_error_seconds': error, 'carrier_sha256': received.hexdigest(),
              'payload_sha256': extracted.hexdigest(), 'fault_profiles': tests,
              'truth_boundary': 'Abstract TV audio passthrough and receiver DMA; no TV OS, Netflix app, DRM, physical eARC or negotiation signalling.'}
    (out / 'tv-ingress.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print('AURORA-SIMICS-TV-INGRESS-PASS')


try:
    main()
    simics.SIM_quit(0)
except Exception:
    traceback.print_exc()
    simics.SIM_quit(1)
