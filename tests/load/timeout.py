#!/usr/bin/env python3
"""timeout local para macOS sin coreutils; TERM coordinado y KILL tras 60 s."""
import os
import signal
import subprocess
import sys


def run(seconds, args):
    child = subprocess.Popen(args, start_new_session=True)
    def forward(signum, frame):
        os.killpg(child.pid, signum)
    previous = {s: signal.signal(s, forward) for s in (signal.SIGTERM, signal.SIGINT)}
    try:
        try:
            return child.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=60)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
            return 124
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


if __name__ == '__main__':
    raise SystemExit(run(float(sys.argv[1]), sys.argv[2:]))
