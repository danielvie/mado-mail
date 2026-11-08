#!/usr/bin/env python3
"""Bounded, isolated native check. Terminate only the process group we launch."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
output = root / "target/macos-reader-check.json"
if sys.platform != "darwin":
    sys.exit("This check requires macOS; Windows checks remain Windows-only.")
output.unlink(missing_ok=True)
with tempfile.TemporaryDirectory(prefix="mado-reader-check-") as home:
    env = dict(os.environ, HOME=home)
    process = subprocess.Popen([str(root / "target/release/mado-mail"), "--reader-check"],
                               cwd=root, env=env, start_new_session=True)
    try:
        code = process.wait(timeout=45)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
        sys.exit("WK reader check timed out; no passing result.")
if code != 0:
    sys.exit(code)
if not output.exists() or json.loads(output.read_text()).get("passed") is not True:
    sys.exit("Native check exited without a passing result.")
print(f"PASS: {output.relative_to(root)}")
