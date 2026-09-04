#!/usr/bin/env python3
import os
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time

MAGIC = 0x30435341
VERSION = 1
REGISTER = 1
PRESENT = 2
GRANT = 4
CONTROL = 7
FORMAT = 8
STATUS = 10
LOCAL = 2
CONTROL_CLIENT = 255
CTRL_MUTE = 1
CTRL_GAIN = 2
CTRL_LIPSYNC = 3
CTRL_STANDBY = 4
FMT_PCM_STEREO = 1
MESSAGE = struct.Struct("<IHHHHIQII")


def pack(kind, source, data0=0, data1=0):
    return MESSAGE.pack(MAGIC, VERSION, kind, source, 0, 0,
                        data0 & 0xFFFFFFFFFFFFFFFF, data1, 0)


def recv_msg(sock, timeout=1.0):
    sock.settimeout(timeout)
    raw = sock.recv(MESSAGE.size)
    assert len(raw) == MESSAGE.size
    f = MESSAGE.unpack(raw)
    assert f[0] == MAGIC and f[1] == VERSION
    return {"kind": f[2], "source": f[3], "data0": f[6], "data1": f[7]}


def connect(path):
    deadline = time.monotonic() + 2.0
    while True:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        try:
            s.connect(path)
            return s
        except (FileNotFoundError, ConnectionRefusedError):
            s.close()
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.01)


def terminate(proc):
    if proc is None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=2)


def run_ctl(binary, env, *args):
    return subprocess.run([binary, *args], env=env, check=True,
                          text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE)


def expect_control(source_sock, control, value):
    msg = recv_msg(source_sock)
    assert msg["kind"] == CONTROL and msg["source"] == LOCAL, msg
    assert msg["data1"] == control and msg["data0"] == value, msg


def run_no_ack_server(path, ready, errors):
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET)
    try:
        listener.bind(path)
        listener.listen(1)
        ready.set()
        conn, _ = listener.accept()
        try:
            register = recv_msg(conn)
            assert register["kind"] == REGISTER
            assert register["source"] == CONTROL_CLIENT
            conn.sendall(pack(STATUS, CONTROL_CLIENT, data0=LOCAL))
            control = recv_msg(conn)
            assert control["kind"] == CONTROL
            assert control["source"] == CONTROL_CLIENT
            assert control["data1"] == CTRL_MUTE
            # Deliberately do not acknowledge the accepted control. The CLI
            # must time out and return failure instead of assuming delivery.
            time.sleep(0.65)
        finally:
            conn.close()
    except BaseException as exc:  # propagated after thread join
        errors.append(exc)
        ready.set()
    finally:
        listener.close()


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: test_source_control_cli.py MANAGER CTL")
    manager_bin, ctl_bin = sys.argv[1:]

    with tempfile.TemporaryDirectory(prefix="aurora-source-ctl-") as td:
        path = os.path.join(td, "manager.sock")
        env = os.environ.copy()
        env["AURORA_SOURCE_MANAGER_SOCKET"] = path
        manager = subprocess.Popen([manager_bin], env=env,
                                   stdout=subprocess.DEVNULL,
                                   stderr=subprocess.PIPE, text=True)
        local = None
        try:
            local = connect(path)
            local.sendall(pack(REGISTER, LOCAL))
            status = recv_msg(local)
            assert status["kind"] == STATUS
            local.sendall(pack(FORMAT, LOCAL, data1=FMT_PCM_STEREO))
            local.sendall(pack(PRESENT, LOCAL))
            grant = recv_msg(local)
            assert grant["kind"] == GRANT
            for _ in range(4):
                assert recv_msg(local)["kind"] == CONTROL

            status_out = run_ctl(ctl_bin, env, "status")
            assert status_out.stdout.strip() == "active_source=local-music"

            run_ctl(ctl_bin, env, "mute", "on")
            expect_control(local, CTRL_MUTE, 1)

            run_ctl(ctl_bin, env, "gain-db", "-6.0")
            expect_control(local, CTRL_GAIN, (-6000) & 0xFFFFFFFFFFFFFFFF)

            run_ctl(ctl_bin, env, "lipsync-ms", "25")
            expect_control(local, CTRL_LIPSYNC, 1200)

            run_ctl(ctl_bin, env, "standby", "on")
            expect_control(local, CTRL_STANDBY, 1)

            invalid = subprocess.run([ctl_bin, "gain-db", "3"], env=env,
                                     text=True, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE)
            assert invalid.returncode == 2
        finally:
            if local is not None:
                local.close()
            terminate(manager)
            if manager.returncode not in (0, -15):
                stderr = manager.stderr.read() if manager.stderr else ""
                raise AssertionError(f"manager exited {manager.returncode}: {stderr}")

        # Prove a control write without manager acknowledgement is not reported
        # as success by the one-shot CLI.
        noack_path = os.path.join(td, "noack.sock")
        noack_env = os.environ.copy()
        noack_env["AURORA_SOURCE_MANAGER_SOCKET"] = noack_path
        ready = threading.Event()
        errors = []
        thread = threading.Thread(target=run_no_ack_server,
                                  args=(noack_path, ready, errors), daemon=True)
        thread.start()
        assert ready.wait(timeout=1.0)
        noack = subprocess.run([ctl_bin, "mute", "on"], env=noack_env,
                               text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
        thread.join(timeout=2.0)
        assert not thread.is_alive(), "no-ACK test server did not finish"
        if errors:
            raise errors[0]
        assert noack.returncode == 1, noack
        assert "not acknowledged" in noack.stderr, noack.stderr

    print("source-control CLI status/control-ack/missing-ack tests passed")


if __name__ == "__main__":
    main()