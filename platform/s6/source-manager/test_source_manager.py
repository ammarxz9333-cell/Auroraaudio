#!/usr/bin/env python3
import os
import socket
import struct
import subprocess
import sys
import tempfile
import time

MAGIC = 0x30435341
VERSION = 1
REGISTER = 1
PRESENT = 2
ABSENT = 3
GRANT = 4
REVOKE = 5
QUIESCED = 6
CONTROL = 7
FORMAT = 8
STATUS = 10

HDMI = 1
LOCAL = 2
CONTROL_CLIENT = 255
CTRL_MUTE = 1
CTRL_GAIN = 2
CTRL_LIPSYNC = 3
CTRL_STANDBY = 4
FMT_IEC61937 = 5
FMT_PCM_STEREO = 1

MESSAGE = struct.Struct("<IHHHHIQII")
assert MESSAGE.size == 32


def pack(kind, source, seq=0, data0=0, data1=0, data2=0):
    return MESSAGE.pack(MAGIC, VERSION, kind, source, 0, seq,
                        data0 & 0xFFFFFFFFFFFFFFFF, data1, data2)


def recv_msg(sock, timeout=1.0):
    sock.settimeout(timeout)
    raw = sock.recv(32)
    if len(raw) != 32:
        raise AssertionError(f"short source-manager message: {len(raw)}")
    fields = MESSAGE.unpack(raw)
    if fields[0] != MAGIC or fields[1] != VERSION:
        raise AssertionError(f"bad message header: {fields[:2]}")
    return {
        "kind": fields[2],
        "source": fields[3],
        "sequence": fields[5],
        "data0": fields[6],
        "data1": fields[7],
        "data2": fields[8],
    }


def connect(path):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
    deadline = time.monotonic() + 2.0
    while True:
        try:
            s.connect(path)
            return s
        except FileNotFoundError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.01)


def register(sock, source):
    sock.sendall(pack(REGISTER, source))
    msg = recv_msg(sock)
    assert msg["kind"] == STATUS and msg["source"] == source


def expect_kind(sock, kind, source):
    msg = recv_msg(sock)
    assert msg["kind"] == kind, msg
    assert msg["source"] == source, msg
    return msg


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: test_source_manager.py /path/to/aurora-source-manager")
    binary = sys.argv[1]
    with tempfile.TemporaryDirectory(prefix="aurora-source-") as td:
        path = os.path.join(td, "source.sock")
        env = os.environ.copy()
        env["AURORA_SOURCE_MANAGER_SOCKET"] = path
        proc = subprocess.Popen(
            [binary], env=env, stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, text=True
        )
        local = None
        hdmi = None
        control = None
        try:
            local = connect(path)
            hdmi = connect(path)
            control = connect(path)
            register(local, LOCAL)
            register(hdmi, HDMI)
            register(control, CONTROL_CLIENT)

            local.sendall(pack(FORMAT, LOCAL, data1=FMT_PCM_STEREO))
            local.sendall(pack(PRESENT, LOCAL))
            expect_kind(local, GRANT, LOCAL)
            controls = [recv_msg(local) for _ in range(4)]
            assert [m["kind"] for m in controls] == [CONTROL] * 4
            assert {m["data1"] for m in controls} == {
                CTRL_GAIN, CTRL_LIPSYNC, CTRL_STANDBY, CTRL_MUTE
            }

            hdmi.sendall(pack(FORMAT, HDMI, data1=FMT_IEC61937))
            hdmi.sendall(pack(PRESENT, HDMI))
            revoke = expect_kind(local, REVOKE, LOCAL)
            assert revoke["data0"] == 10

            hdmi.settimeout(0.05)
            try:
                hdmi.recv(32)
                raise AssertionError("HDMI granted before old source quiesced")
            except socket.timeout:
                pass

            local.sendall(pack(QUIESCED, LOCAL))
            expect_kind(hdmi, GRANT, HDMI)
            hdmi_controls = [recv_msg(hdmi) for _ in range(4)]
            assert {m["data1"] for m in hdmi_controls} == {
                CTRL_GAIN, CTRL_LIPSYNC, CTRL_STANDBY, CTRL_MUTE
            }

            control.sendall(pack(CONTROL, CONTROL_CLIENT, data0=1, data1=CTRL_MUTE))
            mute = expect_kind(hdmi, CONTROL, HDMI)
            assert mute["data1"] == CTRL_MUTE and mute["data0"] == 1

            minus_3000_mdb = 0xFFFFFFFFFFFFFFFF - 2999
            control.sendall(pack(CONTROL, CONTROL_CLIENT,
                                 data0=minus_3000_mdb, data1=CTRL_GAIN))
            gain = expect_kind(hdmi, CONTROL, HDMI)
            assert gain["data1"] == CTRL_GAIN

            hdmi.sendall(pack(ABSENT, HDMI))
            expect_kind(hdmi, REVOKE, HDMI)
            hdmi.sendall(pack(QUIESCED, HDMI))
            expect_kind(local, GRANT, LOCAL)
            replay = [recv_msg(local) for _ in range(4)]
            by_control = {m["data1"]: m["data0"] for m in replay}
            assert by_control[CTRL_MUTE] == 1
            assert by_control[CTRL_GAIN] == minus_3000_mdb

            # A broken source must not wedge arbitration forever. Make HDMI
            # present again, then deliberately ignore LOCAL's REVOKE. The
            # watchdog must disconnect LOCAL and grant HDMI after its bounded
            # quiesce deadline.
            hdmi.sendall(pack(PRESENT, HDMI))
            expect_kind(local, REVOKE, LOCAL)
            hdmi.settimeout(1.0)
            watchdog_grant = expect_kind(hdmi, GRANT, HDMI)
            assert watchdog_grant["kind"] == GRANT
            watchdog_controls = [recv_msg(hdmi) for _ in range(4)]
            assert {m["data1"] for m in watchdog_controls} == {
                CTRL_GAIN, CTRL_LIPSYNC, CTRL_STANDBY, CTRL_MUTE
            }
            local.settimeout(0.2)
            assert local.recv(32) == b"", "stalled source control socket stayed open"

        finally:
            for s in (local, hdmi, control):
                if s is not None:
                    s.close()
            proc.terminate()
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)
            if proc.returncode not in (0, -15):
                stderr = proc.stderr.read() if proc.stderr else ""
                raise AssertionError(f"source manager exited {proc.returncode}: {stderr}")
    print("source-manager priority/quiesce/control/watchdog tests passed")


if __name__ == "__main__":
    main()
