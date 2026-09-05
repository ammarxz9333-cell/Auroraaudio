#!/usr/bin/env python3
"""Prove manager -> gate -> dedicated socket -> real DSP sample delay and replay."""
import array
import os
from pathlib import Path
import random
import socket
import subprocess
import sys
import tempfile
import time

from test_source_control_cli import terminate
from test_source_gate import wait_connect, frame as usb_frame, USB_ENCODED, recv_kind


def wait_path(path, proc):
    deadline = time.monotonic() + 10
    while not os.path.exists(path):
        assert proc.poll() is None, "process exited before control endpoint became available"
        if time.monotonic() >= deadline: raise TimeoutError(path)
        time.sleep(0.01)


def main():
    manager_bin, gate_bin, ctl_bin, post_bin = [str(Path(arg).resolve()) for arg in sys.argv[1:]]
    samples = array.array("f", [0.0]) * (24000 * 12)
    rng = random.Random(20260905)
    for frame in range(24000): samples[frame * 12] = rng.uniform(-0.1, 0.1)
    if sys.byteorder != "little": samples.byteswap()
    data = samples.tobytes()
    clean = {k: v for k, v in os.environ.items() if not k.startswith("AURORA_")}
    baseline = subprocess.run([post_bin], input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env=clean, timeout=30)
    assert baseline.returncode == 0, baseline.stderr.decode()
    reference = array.array("f"); reference.frombytes(baseline.stdout)
    if sys.byteorder != "little": reference.byteswap()
    with tempfile.TemporaryDirectory(prefix="aurora-dsp-control-") as td:
        path = lambda name: str(Path(td) / name)
        env = {**clean, "AURORA_SOURCE_MANAGER_SOCKET": path("manager.sock"),
               "AURORA_HDMI_SOURCE_SOCKET": path("hdmi.sock"),
               "AURORA_LOCAL_SOURCE_SOCKET": path("local.sock"),
               "AURORA_USB_BRIDGE_SOCKET_REAL": path("bridge.sock"),
               "AURORA_DSP_CONTROL_SOCKET": path("dsp.sock")}
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        listener.bind(env["AURORA_USB_BRIDGE_SOCKET_REAL"]); listener.listen(1)
        listener.settimeout(10)
        manager = gate = post = bridge = source = None
        try:
            manager = subprocess.Popen([manager_bin], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            wait_path(env["AURORA_SOURCE_MANAGER_SOCKET"], manager)
            gate = subprocess.Popen([gate_bin], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            bridge, _ = listener.accept()
            source = wait_connect(env["AURORA_HDMI_SOURCE_SOCKET"], timeout=10)
            bridge.sendall(usb_frame(USB_ENCODED, b"\x00" * 16))
            recv_kind(source, USB_ENCODED)
            time.sleep(0.2)
            for iteration in range(2):
                # The second launch deliberately encounters the old socket file.
                post = subprocess.Popen([post_bin], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=env)
                wait_path(env["AURORA_DSP_CONTROL_SOCKET"], post)
                if iteration == 0:
                    result = subprocess.run([ctl_bin, "lipsync-ms", "50"], env=env, capture_output=True, timeout=10)
                    assert result.returncode == 0, result.stderr.decode()
                # No second command: the gate must replay the persisted 50 ms.
                time.sleep(0.5)
                output, error = post.communicate(data, timeout=30)
                assert post.returncode == 0, error.decode()
                rendered = array.array("f"); rendered.frombytes(output)
                if sys.byteorder != "little": rendered.byteswap()
                assert len(rendered) == len(reference)
                differences = [abs(rendered[frame * 12] - reference[(frame - 2400) * 12])
                               for frame in range(12000, 18000)]
                assert max(differences) < 1e-5, (iteration, max(differences))
                assert max(abs(rendered[frame * 12] - reference[frame * 12])
                           for frame in range(12000, 12100)) > 0.01
            rejected = subprocess.run([ctl_bin, "lipsync-ms", "501"], env=env, capture_output=True, timeout=10)
            assert rejected.returncode != 0
        finally:
            for proc in (post, gate, manager): terminate(proc)
            if bridge: bridge.close()
            if source: source.close()
            listener.close()
    print("PASS: manager/gate/real DSP applies 2400-frame delay and replays it after restart; oversized delay rejected")


if __name__ == "__main__": main()
