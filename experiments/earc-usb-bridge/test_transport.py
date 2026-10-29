"""Host protocol simulation tests. No physical eARC or Atmos decode is exercised."""
import hashlib
import random
import unittest
from transport import Frame,Flag,Mode,StrictReceiver,TransportError,packetize,reconstruct

class ProtocolTests(unittest.TestCase):
    def test_pcm_8ch_192k_24_bit_exact(self):
        b=random.Random(42).randbytes(8*192000*3)
        encoded=b"".join(p.encode() for p in packetize(b,Mode.PCM))
        self.assertEqual(hashlib.sha256(reconstruct(encoded)).digest(),hashlib.sha256(b).digest())
        self.assertEqual(reconstruct(encoded),b)

    def test_opaque_encoded_transport_bytes(self):
        # These labels are NOT actual Dolby MAT/JOC: arbitrary bytes stay unchanged.
        b=b"NOT-A-REAL-MAT-JOC-BITSTREAM"+bytes(range(256))*2000
        self.assertEqual(reconstruct(b"".join(p.encode() for p in packetize(b,Mode.RAW_IEC_WORDS))),b)

    def test_16_output_channels_are_independent(self):
        frames=48000
        chans=16
        interleaved=bytearray()
        for t in range(frames):
            for ch in range(chans):
                interleaved.extend(((t*37+ch*6571)&0xffffff).to_bytes(3,"little"))
        separated=[bytearray() for _ in range(chans)]
        for pos in range(0,len(interleaved),chans*3):
            for ch in range(chans):
                separated[ch].extend(interleaved[pos+3*ch:pos+3*ch+3])
        restored=bytearray()
        for t in range(frames):
            for ch in range(chans):restored.extend(separated[ch][t*3:t*3+3])
        self.assertEqual(restored,interleaved)
        self.assertNotEqual(separated[0],separated[1])

    def test_crc_payload(self):
        raw=bytearray(Frame(Mode.PCM,1,0,0,0,0,b"abcd").encode())
        raw[-1]^=1
        with self.assertRaisesRegex(TransportError,"payload CRC"):
            Frame.parse(bytes(raw))

    def test_crc_header(self):
        raw=bytearray(Frame(Mode.PCM,1,0,0,0,0,b"abcd").encode())
        raw[10]^=1
        with self.assertRaisesRegex(TransportError,"header CRC"):
            Frame.parse(bytes(raw))

    def test_dropped_frame(self):
        r=StrictReceiver()
        r.accept(Frame(Mode.PCM,1,0,0,0,0,b"aa"))
        with self.assertRaisesRegex(TransportError,"USB frame gap"):
            r.accept(Frame(Mode.PCM,1,2,4,0,0,b"bb"))

    def test_source_gap_with_contiguous_usb_seq(self):
        r=StrictReceiver()
        r.accept(Frame(Mode.PCM,1,0,0,0,0,b"aa"))
        with self.assertRaisesRegex(TransportError,"source byte gap"):
            r.accept(Frame(Mode.PCM,1,1,3,0,0,b"bb"))
        r.accept(Frame(Mode.PCM,1,1,3,0,0,b"bb",Flag.LOSS_BEFORE_CAPTURE))
        self.assertEqual(r.discontinuities,1)

    def test_mode_changes_are_explicit(self):
        r=StrictReceiver()
        r.accept(Frame(Mode.PCM,1,0,0,0,0,b"aa"))
        with self.assertRaisesRegex(TransportError,"mode change"):
            r.accept(Frame(Mode.RAW_IEC_WORDS,1,1,2,0,0,b"bb"))
        r.accept(Frame(Mode.RAW_IEC_WORDS,1,1,2,0,0,b"bb",Flag.MODE_CHANGE))

    def test_sequence_wrap(self):
        r=StrictReceiver()
        r.accept(Frame(Mode.PCM,1,0xffffffff,0,0,0,b"aa"))
        r.accept(Frame(Mode.PCM,1,0,2,0,0,b"bb"))

    def test_truncation_and_bad_flags(self):
        raw=Frame(Mode.PCM,1,0,0,0,0,b"abcd").encode()
        with self.assertRaisesRegex(TransportError,"truncated"):
            Frame.parse(raw[:-1])
        with self.assertRaisesRegex(TransportError,"unknown flags"):
            Frame(Mode.PCM,1,0,0,0,0,b"abcd",Flag(16)).encode()

if __name__=="__main__":unittest.main()
