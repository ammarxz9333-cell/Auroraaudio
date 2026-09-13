"""Aurora abstract audio DMA contract, not a vendor register implementation.

Runs inside Intel Simics 7 using pyobj, real memory-space transactions and
scheduled device events. PCM words are float32 payloads, not electrical TDM.
"""
import hashlib
import math
import struct

import pyobj
import simics

CTRL, STATUS, SOURCE, LENGTH, RATE, CHANNELS, PERIOD, SUBMIT, DONE, IRQ, ERROR = range(0, 44, 4)
RUN, FAULT = 1, 2
RAM_START, RAM_END = 0x10000000, 0x20000000
TDM_LANES = 2
TDM_SLOTS_PER_LANE = 8
TDM_SLOT_BYTES = 4
FSYNC_HZ = 48000
BCLK_HZ = FSYNC_HZ * TDM_SLOTS_PER_LANE * 32
MCLK_HZ = 24576000


class aurora_audio_dma(pyobj.ConfObject):
    """Two-entry audio DMA FIFO with period IRQ and latched fail-closed state."""
    _class_desc = "Aurora audio I/O timing model"

    class target_mem(pyobj.SimpleAttribute(None, 'o', simics.Sim_Attr_Required)):
        """DMA address space."""

    def _initialize(self):
        super()._initialize()
        self.ev = simics.SIM_register_event(
            'aurora-period', None, simics.Sim_EC_Notsaved,
            lambda obj, data: obj.object_data.tick(), None, None, None, None)
        self.output_sink = None
        self.reset()

    def cancel(self):
        simics.SIM_event_cancel_time(simics.SIM_object_clock(self.obj), self.ev,
                                     self.obj, None, None)

    def reset(self):
        if self.output_sink is not None:
            self.output_sink.mute()
        self.r = {CTRL: 0, STATUS: 0, SOURCE: RAM_START, LENGTH: 11520,
                  RATE: 48000, CHANNELS: 12, PERIOD: 240, SUBMIT: 0,
                  DONE: 0, IRQ: 0, ERROR: 0}
        self.pending = []
        self.frames = 0
        self.times = []
        self.pcm_hash = hashlib.sha256()
        self.tdm_hash = hashlib.sha256()
        self.tdm_lane_hashes = [hashlib.sha256(), hashlib.sha256()]
        self.last_tdm = b''
        self.last_tdm_lanes = (b'', b'')

    def fail(self, code):
        self.cancel()
        self.r[STATUS] = FAULT
        self.r[CTRL] = 0
        self.r[ERROR] = code
        self.r[IRQ] |= 2
        self.pending.clear()
        self.last_tdm = bytes(240 * 16 * 4)
        self.last_tdm_lanes = (bytes(240 * 8 * 4), bytes(240 * 8 * 4))
        if self.output_sink is not None:
            self.output_sink.mute()

    def schedule(self):
        simics.SIM_event_post_time(simics.SIM_object_clock(self.obj), self.ev,
                                  self.obj, self.r[PERIOD] / self.r[RATE], None)

    def write(self, offset, value):
        if offset == CTRL and value == 2:
            self.cancel()
            self.reset()
            return
        if self.r[STATUS] == FAULT:
            return
        if offset == IRQ:
            self.r[IRQ] &= ~value  # write-one-to-clear
        elif offset == CTRL:
            if value == 0:
                self.cancel()
                self.r[STATUS] = self.r[CTRL] = 0
                self.pending.clear()
                self.last_tdm = bytes(240 * 64)
                self.last_tdm_lanes = (bytes(240 * 32), bytes(240 * 32))
                if self.output_sink is not None:
                    self.output_sink.mute()
            elif value == 1 and self.r[STATUS] != RUN:
                if (self.r[RATE], self.r[CHANNELS], self.r[PERIOD]) != (48000, 12, 240):
                    self.fail(1)
                else:
                    self.r[STATUS] = self.r[CTRL] = RUN
                    self.schedule()
            else:
                self.fail(2)
        elif offset == SUBMIT:
            src, length = self.r[SOURCE], self.r[LENGTH]
            if value != 1 or length != self.r[PERIOD] * 48:
                self.fail(3)
            elif src % 4 or not RAM_START <= src or src + length > RAM_END:
                self.fail(4)
            elif len(self.pending) == 2:
                self.fail(5)
            else:
                self.pending.append((src, length))
        elif offset in (SOURCE, LENGTH, RATE, CHANNELS, PERIOD):
            if self.r[STATUS] == RUN and offset in (RATE, CHANNELS, PERIOD):
                self.fail(6)
            else:
                self.r[offset] = value
        else:
            self.fail(7)

    def tick(self):
        if self.r[STATUS] != RUN:
            return
        if not self.pending:
            self.fail(8)
            return
        src, length = self.pending.pop(0)
        try:
            pcm = b''.join(bytes(self.target_mem.val.iface.memory_space.read(
                self.obj, src + offset, min(1024, length - offset), 0))
                for offset in range(0, length, 1024))
        except Exception as exc:
            print('Aurora DMA transaction failed:', repr(exc))
            self.fail(9)
            return
        if not all(math.isfinite(v[0]) for v in struct.iter_unpack('<f', pcm)):
            self.fail(10)
            return
        lane0 = b''.join(pcm[i:i + 32] for i in range(0, length, 48))
        lane1 = b''.join(pcm[i + 32:i + 48] + bytes(16) for i in range(0, length, 48))
        tdm = b''.join(lane0[i:i + 32] + lane1[i:i + 32]
                       for i in range(0, len(lane0), 32))
        self.last_tdm = tdm
        self.last_tdm_lanes = (lane0, lane1)
        if self.output_sink is not None:
            if hasattr(self.output_sink, 'consume_lanes'):
                self.output_sink.consume_lanes(lane0, lane1)
            else:
                self.output_sink.consume(tdm)
        self.pcm_hash.update(pcm)
        self.tdm_hash.update(tdm)
        self.tdm_lane_hashes[0].update(lane0)
        self.tdm_lane_hashes[1].update(lane1)
        self.frames += self.r[PERIOD]
        self.r[DONE] += 1
        self.r[IRQ] |= 1
        self.times.append(simics.SIM_time(simics.SIM_object_clock(self.obj)))
        self.schedule()

    class regs(pyobj.PortObject):
        """32-bit little-endian registers; all other transactions fault."""
        namespace = 'bank'

        class io_memory(pyobj.Interface):
            def operation(self, mop, info):
                dev = self._up._up
                offset = simics.SIM_get_mem_op_physical_address(mop) + info.start - info.base
                if simics.SIM_get_mem_op_size(mop) != 4 or offset not in dev.r:
                    dev.fail(7)
                    return simics.Sim_PE_IO_Error
                if simics.SIM_mem_op_is_read(mop):
                    simics.SIM_set_mem_op_value_le(mop, dev.r[offset])
                else:
                    dev.write(offset, simics.SIM_get_mem_op_value_le(mop))
                return simics.Sim_PE_No_Exception
