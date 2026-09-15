#!/usr/bin/env python3
"""Independent OBR directional checks for Aurora's prepared SOFA PCM path."""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import wave

from binaural_7_1_4_differential import read_stereo_metrics

OBR = "478dc7c752d5eccae534635139ff0253eee3a14a"
SOFAR = "06a629292689e99841e5dacaa25c4c6298616ca6"
MYSOFA = "da9e4adc619ee3d1ae5e68da3ed14aa5e60b3ec1"
SOFA_HASH = "2768ac841213a7ae11d1ea7fd0f25a69b39216102dc5dd913ea6ba0f0dc57e28"
NAMES = {"front", "back", "left", "right", "up", "down", "yaw-left", "yaw-right", "pitch-up", "roll-up"}
NAMES |= {f"hoa{o}-{p}" for o in range(1, 4) for p in ("left", "right", "yaw-right")}


def harmonics(p):
    x, y, z = p
    a, b, c, d = math.sqrt(3), math.sqrt(15), math.sqrt(3/8), math.sqrt(5/8)
    return [1,y,z,x,a*x*y,a*y*z,(3*z*z-1)/2,a*x*z,a*(x*x-y*y)/2,
            d*y*(3*x*x-y*y),b*x*y*z,c*y*(5*z*z-1),z*(5*z*z-3)/2,
            c*x*(5*z*z-1),b*z*(x*x-y*y)/2,d*x*(x*x-3*y*y)]


def validate_native(data):
    if data.get("schema_version") != 1 or data.get("verdict") != "pass":
        raise ValueError("invalid native evidence")
    cases = data.get("cases", [])
    if len(cases) != len(NAMES) or {c["name"] for c in cases} != NAMES:
        raise ValueError("missing/duplicate/unexpected cases")
    for c in cases:
        pcm = c["pcm"]
        if len(pcm) != 4096 or not all(isinstance(v, (int, float)) and math.isfinite(v) for v in pcm):
            raise ValueError("invalid PCM")
        left = sum(v*v for v in pcm[::2])
        right = sum(v*v for v in pcm[1::2])
        if left+right <= 1e-12 or not 0 <= c["max_absolute_pcm_error"] <= 1e-5:
            raise ValueError("silent or mismatched native PCM")
        bias = 10*math.log10((left+1e-20)/(right+1e-20))
        y = c["head_direction"][1]
        if abs(y) > 0.3 and bias*y <= 0:
            raise ValueError("native directional semantics failed")
    return {c["name"]: c for c in cases}


def main():
    p = argparse.ArgumentParser()
    for arg in ("native", "obr-root", "sofar-root", "sofa", "work-dir", "output"):
        p.add_argument("--"+arg, type=Path, required=True)
    a = p.parse_args()
    for root, pin in ((a.obr_root, OBR), (a.sofar_root, SOFAR),
                      (a.sofar_root/"libmysofa-sys/libmysofa", MYSOFA)):
        actual = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
        if actual != pin:
            raise ValueError("reference pin mismatch")
    if hashlib.sha256(a.sofa.read_bytes()).hexdigest() != SOFA_HASH:
        raise ValueError("SOFA dataset hash mismatch")
    native = validate_native(json.loads(a.native.read_text()))
    a.work_dir.mkdir(parents=True, exist_ok=True)
    checks = []
    for name, case in native.items():
        xyz = case["head_direction"]
        order = case["hoa_order"]
        weights = harmonics(xyz)[:(order+1)**2] if order else [1]
        source = a.work_dir/f"{name}-input.wav"
        output = a.work_dir/f"{name}-output.wav"
        samples = array.array("h", [0]) * (2048*len(weights))
        for i, weight in enumerate(weights):
            samples[256*len(weights)+i] = round(weight*8192)
        if sys.byteorder != "little":
            samples.byteswap()
        with wave.open(str(source), "wb") as w:
            w.setnchannels(len(weights)); w.setsampwidth(2); w.setframerate(48000)
            w.writeframes(samples.tobytes())
        command = [str(a.obr_root/"bazel-bin/obr/cli/obr_cli"),
                   f"--input_type={str(order)+'OA' if order else 'OBA'}",
                   "--filter_type=Direct", "--buffer_size=256",
                   f"--input_file={source}", f"--output_file={output}"]
        if not order:
            metadata = a.work_dir/f"{name}.textproto"
            azimuth = math.degrees(math.atan2(xyz[1], xyz[0]))
            elevation = math.degrees(math.asin(max(-1,min(1,xyz[2]))))
            metadata.write_text(f"source {{ input_channel: 0 gain: 1 azimuth: {azimuth} elevation: {elevation} distance: 1 }}\n")
            command.append(f"--oba_metadata_file={metadata}")
        subprocess.run(command, check=True, capture_output=True, text=True, timeout=60)
        metrics = read_stereo_metrics(output)
        if metrics["frames"] != 2048 or metrics["channels"] != 2 or metrics["total_energy"] <= 0:
            raise ValueError("OBR output integrity failed")
        if abs(xyz[1]) > 0.3 and metrics["left_right_power_bias_db"]*xyz[1] <= 0:
            raise ValueError(f"OBR directional mismatch: {name}")
        checks.append({"name":name,"obr_bias_db":metrics["left_right_power_bias_db"],
                       "native_bias_db":case["bias_db"],"pass":True})
    discrimination = []
    for first, second in (("front","back"),("up","down")):
        x, y = native[first]["pcm"], native[second]["pcm"]
        distance = math.sqrt(sum((i-j)**2 for i,j in zip(x,y))/max(sum(i*i for i in x),1e-20))
        if distance < 0.05:
            raise ValueError("non-discriminating SOFA transfer responses")
        discrimination.append({"pair":[first,second],"relative_pcm_distance":distance,
                               "boundary":"transfer-function discrimination, not listener perception"})
    a.output.write_text(json.dumps({"verdict":"pass","checks":checks,"discrimination":discrimination,
        "references":{"obr":OBR,"sofar":SOFAR,"libmysofa":MYSOFA,"sofa_sha256":SOFA_HASH},
        "truth_boundary":"Software reference PCM only. HOA uses prepared quadrature filters and directional plane waves; OBR rotation comparison uses equivalent head-coordinate input. No perceptual parity, physical tracker/DAC/eARC/acoustic or certification proof."},indent=2)+"\n")


if __name__ == "__main__":
    main()
