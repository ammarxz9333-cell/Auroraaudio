"""Experimental, opaque byte framing for Aurora. NOT USB/eARC firmware."""
from dataclasses import dataclass
from enum import IntEnum, IntFlag
import struct
import zlib

MAGIC=b"AUR1"
VERSION=1
MAX_PAYLOAD=16384
_HEADER=struct.Struct("<4sBBHIIQQIIII")

class Mode(IntEnum):
    PCM=1
    RAW_IEC_WORDS=2
    SIDEBAND=3

class Flag(IntFlag):
    NONE=0
    DISCONTINUITY=1
    SOURCE_RELOCK=2
    MODE_CHANGE=4
    LOSS_BEFORE_CAPTURE=8

class TransportError(ValueError):
    pass

@dataclass(frozen=True)
class Frame:
    mode:Mode
    stream_id:int
    seq:int
    source_byte_offset:int
    monotonic_ns:int
    status_id:int
    payload:bytes
    flags:Flag=Flag.NONE

    def encode(self)->bytes:
        if len(self.payload)>MAX_PAYLOAD: raise TransportError("payload oversize")
        if int(self.flags)&~15: raise TransportError("unknown flags")
        try:
            header=_HEADER.pack(MAGIC,VERSION,int(self.mode),int(self.flags),
                self.stream_id,self.seq,self.source_byte_offset,self.monotonic_ns,
                self.status_id,len(self.payload),zlib.crc32(self.payload),0)
        except struct.error as err: raise TransportError(str(err)) from err
        return header[:-4]+struct.pack("<I",zlib.crc32(header[:-4]))+self.payload

    @staticmethod
    def parse(data:bytes):
        nhead=_HEADER.size
        if len(data)<nhead:raise TransportError("short header")
        magic,version,mode,flags,sid,seq,offset,clock,status,length,crc,hcrc=_HEADER.unpack_from(data)
        if magic!=MAGIC or version!=VERSION:raise TransportError("magic/version")
        if zlib.crc32(data[:nhead-4])!=hcrc:raise TransportError("header CRC")
        if flags&~15:raise TransportError("unknown flags")
        if length>MAX_PAYLOAD or len(data)<nhead+length:raise TransportError("truncated payload")
        payload=data[nhead:nhead+length]
        if zlib.crc32(payload)!=crc:raise TransportError("payload CRC")
        try: mode=Mode(mode)
        except ValueError as err:raise TransportError("unknown mode") from err
        return Frame(mode,sid,seq,offset,clock,status,payload,Flag(flags)),data[nhead+length:]

class StrictReceiver:
    """Reject CRC damage, gaps and unflagged changes; never repair concealed losses."""
    def __init__(self):
        self.last={}
        self.discontinuities=0

    def accept(self,f:Frame):
        old=self.last.get(f.stream_id)
        if old:
            if f.seq!=((old.seq+1)&0xffffffff):
                raise TransportError("USB frame gap/reorder")
            if f.source_byte_offset!=old.source_byte_offset+len(old.payload) and not f.flags & (
                Flag.DISCONTINUITY|Flag.SOURCE_RELOCK|Flag.LOSS_BEFORE_CAPTURE):
                raise TransportError("source byte gap")
            if f.mode!=old.mode and not f.flags & Flag.MODE_CHANGE:
                raise TransportError("unflagged source mode change")
        if f.flags & (Flag.DISCONTINUITY|Flag.SOURCE_RELOCK|Flag.LOSS_BEFORE_CAPTURE):
            self.discontinuities+=1
        self.last[f.stream_id]=f

def packetize(payload:bytes,mode:Mode,chunk:int=8192):
    if not 1<=chunk<=MAX_PAYLOAD:raise ValueError("chunk")
    for off in range(0,len(payload),chunk):
        yield Frame(mode,1,off//chunk,off,0,0,payload[off:off+chunk])

def reconstruct(data:bytes):
    host=StrictReceiver()
    out=bytearray()
    while data:
        frame,data=Frame.parse(data)
        host.accept(frame)
        out.extend(frame.payload)
    return bytes(out)
