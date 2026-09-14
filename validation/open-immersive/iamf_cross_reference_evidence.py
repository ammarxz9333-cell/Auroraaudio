#!/usr/bin/env python3
from __future__ import annotations
import argparse, hashlib, json, math, pathlib, struct

def fail(message: str) -> None:
    raise SystemExit(f"iamf-cross-reference: {message}")

def load_f32(path: pathlib.Path, channels: int) -> list[float]:
    data = path.read_bytes()
    frame_bytes = 4 * channels
    if not data:
        fail(f"{path}: PCM is empty")
    if len(data) % frame_bytes:
        fail(f"{path}: PCM byte length is not frame aligned for {channels} channels")
    samples = list(struct.unpack("<" + "f" * (len(data) // 4), data))
    if not all(math.isfinite(value) for value in samples):
        fail(f"{path}: PCM contains NaN/Inf")
    if not any(abs(value) > 1e-10 for value in samples):
        fail(f"{path}: PCM is silent")
    return samples

def stats(samples: list[float], channels: int) -> dict:
    frames = len(samples) // channels
    per_channel, energies = [], []
    for channel in range(channels):
        values = samples[channel::channels]
        energy = sum(value * value for value in values)
        energies.append(energy)
        rms = math.sqrt(energy / len(values)) if values else 0.0
        peak = max((abs(value) for value in values), default=0.0)
        mean = sum(values) / len(values) if values else 0.0
        per_channel.append({"channel": channel, "rms": rms, "rms_dbfs": 20.0*math.log10(rms) if rms>0 else None, "peak": peak, "mean": mean})
    total_energy = sum(energies)
    for i,e in enumerate(energies):
        per_channel[i]["energy_fraction"] = e/total_energy if total_energy>0 else 0.0
    return {"frames":frames, "samples":len(samples), "peak":max(abs(v) for v in samples), "channels":per_channel}

def normalized_correlation(left, right, channels, channel):
    a=left[channel::channels]; b=right[channel::channels]; count=min(len(a),len(b))
    if not count: return None
    a=a[:count]; b=b[:count]
    dot=sum(x*y for x,y in zip(a,b)); aa=sum(x*x for x in a); bb=sum(y*y for y in b)
    if aa<=0 or bb<=0: return None
    return dot/math.sqrt(aa*bb)

def compare(args):
    config=json.loads(args.config.read_text())
    acceptance=config["acceptance"]
    if args.sample_rate != int(acceptance["sample_rate"]): fail("sample rate mismatch")
    if args.channels != int(acceptance["channels"]): fail("channel count mismatch")
    left=load_f32(args.iamf_tools_pcm,args.channels); right=load_f32(args.libiamf_pcm,args.channels)
    ls=stats(left,args.channels); rs=stats(right,args.channels)
    frame_delta=abs(ls["frames"]-rs["frames"])
    if frame_delta > int(acceptance["max_frame_delta"]): fail(f"frame delta {frame_delta} too large")
    metrics=[]
    for i in range(args.channels):
        l=ls["channels"][i]; r=rs["channels"][i]
        rms_delta_db=20*math.log10(l["rms"]/r["rms"]) if l["rms"]>0 and r["rms"]>0 else None
        metrics.append({"channel":i,"rms_delta_db":rms_delta_db,"energy_fraction_delta":l["energy_fraction"]-r["energy_fraction"],"normalized_correlation":normalized_correlation(left,right,args.channels,i)})
    report={"schema_version":1,"status":"pass","fixture":config["fixture"],"pins":config["pins"],"sample_rate":args.sample_rate,"channels":args.channels,"sample_identity_required":False,"iamf_tools":{"pcm_sha256":hashlib.sha256(args.iamf_tools_pcm.read_bytes()).hexdigest(),**ls},"libiamf":{"pcm_sha256":hashlib.sha256(args.libiamf_pcm.read_bytes()).hexdigest(),**rs},"differential":{"frame_delta":frame_delta,"duration_delta_ms":1000*frame_delta/args.sample_rate,"channel_metrics":metrics},"truth_boundary":config["truth_boundary"]}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    return report

def self_test():
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        root=pathlib.Path(td); left=root/"l.f32"; right=root/"r.f32"; cfg=root/"c.json"; out=root/"o.json"
        a=[]; b=[]
        for i in range(64):
            v=.1*math.sin(i*.2); a += [v,-v]; b += [v*.99,-v*1.01]
        left.write_bytes(struct.pack("<"+"f"*len(a),*a)); right.write_bytes(struct.pack("<"+"f"*len(b),*b))
        cfg.write_text(json.dumps({"fixture":{"path":"synthetic"},"pins":{"iamf_tools":"x","libiamf":"y"},"acceptance":{"sample_rate":48000,"channels":2,"max_frame_delta":0},"truth_boundary":"self-test"}))
        args=argparse.Namespace(iamf_tools_pcm=left,libiamf_pcm=right,config=cfg,output=out,sample_rate=48000,channels=2)
        report=compare(args); assert report["status"]=="pass"; assert report["differential"]["frame_delta"]==0
    print("IAMF-CROSS-REFERENCE-SELF-TEST-PASS")

def main():
    parser=argparse.ArgumentParser(); subs=parser.add_subparsers(dest="command",required=True); subs.add_parser("self-test")
    cp=subs.add_parser("compare"); cp.add_argument("iamf_tools_pcm",type=pathlib.Path); cp.add_argument("libiamf_pcm",type=pathlib.Path); cp.add_argument("config",type=pathlib.Path); cp.add_argument("output",type=pathlib.Path); cp.add_argument("--sample-rate",type=int,default=48000); cp.add_argument("--channels",type=int,default=2)
    args=parser.parse_args()
    if args.command=="self-test": self_test()
    else: print(json.dumps(compare(args),indent=2,sort_keys=True))

if __name__=="__main__": main()
