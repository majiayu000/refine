#!/usr/bin/env python3
"""Weekly LLM command backed by the Cursor agent CLI.

Install as ~/.refine/bin/weekly-llm (chmod +x) to route the weekly job's
ingest and insights through Cursor instead of the API provider. Reads the
prompt on stdin and prints the answer on stdout.

The agent runs read-only (--mode ask) in an empty directory so it cannot touch
any project files. Its stream of events is watched: a call that produces no
event for REFINE_CURSOR_IDLE_SECONDS (default 120) is treated as hung and
killed, while a long answer that keeps streaming is allowed to finish.
"""

import json
import os
import queue
import signal
import subprocess
import sys
import threading

HOME = os.path.expanduser("~")
CURSOR_BIN = os.environ.get("REFINE_CURSOR_BIN", os.path.join(HOME, ".local/bin/cr"))
WORKDIR = os.environ.get("REFINE_CURSOR_WORKDIR", os.path.join(HOME, ".refine/cursor-llm-workdir"))
IDLE_SECONDS = float(os.environ.get("REFINE_CURSOR_IDLE_SECONDS", "120"))


def main() -> int:
    prompt = sys.stdin.read()
    os.makedirs(WORKDIR, exist_ok=True)
    args = [CURSOR_BIN, "-p", "--trust", "--mode", "ask", "--output-format", "stream-json"]
    if os.environ.get("REFINE_CURSOR_MODEL"):
        args += ["--model", os.environ["REFINE_CURSOR_MODEL"]]
    args.append(prompt)
    proc = subprocess.Popen(
        args,
        cwd=WORKDIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )

    events: "queue.Queue[str | None]" = queue.Queue()
    stderr_tail: list[str] = []

    def read_stdout() -> None:
        for line in proc.stdout:
            events.put(line)
        events.put(None)

    def read_stderr() -> None:
        for line in proc.stderr:
            stderr_tail.append(line)
            del stderr_tail[:-20]

    threading.Thread(target=read_stdout, daemon=True).start()
    threading.Thread(target=read_stderr, daemon=True).start()

    while True:
        try:
            line = events.get(timeout=IDLE_SECONDS)
        except queue.Empty:
            os.killpg(proc.pid, signal.SIGKILL)
            print(f"cursor agent idle for {IDLE_SECONDS:.0f}s; killed as hung", file=sys.stderr)
            return 124
        if line is None:
            break
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") != "result":
            continue
        proc.wait()
        if event.get("is_error") or event.get("subtype") != "success":
            print(f"cursor agent error: {event.get('result') or event}", file=sys.stderr)
            return 1
        print(event.get("result", ""))
        return 0

    rc = proc.wait()
    sys.stderr.write("".join(stderr_tail) or "cursor agent exited without a result\n")
    return rc or 1


if __name__ == "__main__":
    sys.exit(main())
