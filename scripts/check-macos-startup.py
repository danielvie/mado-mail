#!/usr/bin/env python3
"""Smoke-launch task run/demo with a temporary HOME; inspect only our own windows."""
import ctypes as c
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import sys
import tempfile
import time

if sys.platform != "darwin" or sys.argv[1:] not in (["run"], ["demo"]):
    sys.exit("Usage on macOS: python3 scripts/check-macos-startup.py run|demo")
root = Path(__file__).resolve().parents[1]
cf = c.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
cg = c.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
cf.CFStringCreateWithCString.argtypes = [c.c_void_p, c.c_char_p, c.c_uint]
cf.CFStringCreateWithCString.restype = c.c_void_p
cf.CFArrayGetCount.argtypes = [c.c_void_p]
cf.CFArrayGetCount.restype = c.c_long
cf.CFArrayGetValueAtIndex.argtypes = [c.c_void_p, c.c_long]
cf.CFArrayGetValueAtIndex.restype = c.c_void_p
cf.CFDictionaryGetValue.argtypes = [c.c_void_p, c.c_void_p]
cf.CFDictionaryGetValue.restype = c.c_void_p
cf.CFNumberGetValue.argtypes = [c.c_void_p, c.c_int, c.c_void_p]
cf.CFPropertyListCreateData.argtypes = [c.c_void_p, c.c_void_p, c.c_long, c.c_ulong, c.c_void_p]
cf.CFPropertyListCreateData.restype = c.c_void_p
cf.CFDataGetLength.argtypes = [c.c_void_p]
cf.CFDataGetLength.restype = c.c_long
cf.CFDataGetBytePtr.argtypes = [c.c_void_p]
cf.CFDataGetBytePtr.restype = c.c_void_p
cf.CFRelease.argtypes = [c.c_void_p]
cg.CGWindowListCopyWindowInfo.argtypes = [c.c_uint, c.c_uint]
cg.CGWindowListCopyWindowInfo.restype = c.c_void_p
pid_key = cf.CFStringCreateWithCString(None, b"kCGWindowOwnerPID", 0x08000100)


def windows(pids):
    array = cg.CGWindowListCopyWindowInfo(1, 0)  # on-screen only
    found = []
    try:
        for index in range(cf.CFArrayGetCount(array)):
            item = cf.CFArrayGetValueAtIndex(array, index)
            number = cf.CFDictionaryGetValue(item, pid_key)
            pid = c.c_int()
            if not number or not cf.CFNumberGetValue(number, 9, c.byref(pid)) or pid.value not in pids:
                continue
            data = cf.CFPropertyListCreateData(None, item, 100, 0, None)
            try:
                own = plistlib.loads(c.string_at(cf.CFDataGetBytePtr(data), cf.CFDataGetLength(data)))
                if own.get("kCGWindowLayer") == 0:
                    found.append({key: own.get(key) for key in ("kCGWindowOwnerPID", "kCGWindowBounds", "kCGWindowName")})
            finally:
                cf.CFRelease(data)
    finally:
        cf.CFRelease(array)
    return found


original_home = Path.home()
with tempfile.TemporaryDirectory(prefix="mado-startup-") as home:
    env = dict(os.environ, HOME=home,
               CARGO_HOME=os.environ.get("CARGO_HOME", str(original_home / ".cargo")),
               RUSTUP_HOME=os.environ.get("RUSTUP_HOME", str(original_home / ".rustup")))
    with (root / f"target/startup-{sys.argv[1]}.log").open("w") as log:
        process = subprocess.Popen(["task", sys.argv[1]], cwd=root, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + 30
            found = []
            while process.poll() is None and time.monotonic() < deadline:
                rows = subprocess.check_output(["ps", "-axo", "pid=,pgid="], text=True).splitlines()
                pids = {int(row.split()[0]) for row in rows if int(row.split()[1]) == process.pid}
                found = windows(pids)
                if found:
                    time.sleep(2)
                    if process.poll() is None:
                        print(f"PASS task {sys.argv[1]}: own on-screen window {found}; temporary HOME, no Gmail connection")
                        break
                time.sleep(.2)
            else:
                sys.exit(f"No live on-screen app window; inspect target/startup-{sys.argv[1]}.log")
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=5)
cf.CFRelease(pid_key)
