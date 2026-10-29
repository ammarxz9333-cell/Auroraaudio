"""A real (non-Atmos) E-AC-3 IEC61937 byte-preservation test using FFmpeg.
Requires ffmpeg and ffprobe; no HDMI/eARC device or MAT/JOC data is involved.
Run from this directory: python3 verify_eac3.py
"""
import hashlib
import json
import pathlib
import subprocess
import tempfile
from transport import Mode,packetize,reconstruct

def run(*cmd):
    return subprocess.run(cmd,check=True,capture_output=True,text=True).stdout

def main():
    with tempfile.TemporaryDirectory() as folder:
        root=pathlib.Path(folder)
        eac3=root/"source.eac3"
        iec=root/"source.spdif"
        out=root/"recovered.spdif"
        run("ffmpeg","-hide_banner","-loglevel","error","-y",
            "-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=1",
            "-af","pan=5.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0",
            "-c:a","eac3","-b:a","640k","-f","eac3",str(eac3))
        run("ffmpeg","-hide_banner","-loglevel","error","-y",
            "-f","eac3","-i",str(eac3),"-c:a","copy","-f","spdif",str(iec))
        original=iec.read_bytes()
        framed=b"".join(f.encode() for f in packetize(original,Mode.RAW_IEC_WORDS))
        recovered=reconstruct(framed)
        if original!=recovered:raise AssertionError("payload bytes changed")
        out.write_bytes(recovered)
        before=json.loads(run("ffprobe","-v","error","-f","spdif","-show_entries",
                              "stream=codec_name,channels,sample_rate","-of","json",str(iec)))
        after=json.loads(run("ffprobe","-v","error","-f","spdif","-show_entries",
                             "stream=codec_name,channels,sample_rate","-of","json",str(out)))
        if before!=after:raise AssertionError("ffprobe codec metadata changed")
        print(json.dumps({"software_only":True,"format":after["streams"][0],
            "bytes":len(original),"SHA256":hashlib.sha256(recovered).hexdigest(),
            "bit_exact":True,"joc_mat_earc_tested":False},indent=2))
if __name__=="__main__":main()
