"""Starts EasyProxy for REFEREE, bound to loopback and tied to REFEREE's lifetime.

EasyProxy's own main() binds 0.0.0.0, which would put an open proxy on the LAN,
so this repeats main() with a loopback host. A watchdog thread exits when the
parent process is gone, because nothing else stops this child if REFEREE is
killed.
"""
import logging
import os
import sys
import threading
import time

APP_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app")


def exit_with_parent(parent_pid):
    import psutil

    try:
        parent = psutil.Process(parent_pid)
    except psutil.Error:
        os._exit(0)
    while True:
        time.sleep(2)
        # is_running() also compares creation times, so a reused PID counts as gone.
        if not parent.is_running():
            os._exit(0)


def main():
    parent_pid = int(os.environ.get("REFEREE_PARENT_PID") or 0)
    if parent_pid:
        threading.Thread(target=exit_with_parent, args=(parent_pid,), daemon=True).start()

    os.chdir(APP_DIR)
    sys.path.insert(0, APP_DIR)
    import app as easyproxy
    from aiohttp import web

    if sys.platform == "win32":
        # Same as EasyProxy's main(): silences ConnectionResetError spam.
        logging.getLogger("asyncio").setLevel(logging.CRITICAL)
    web.run_app(easyproxy.app, host="127.0.0.1", port=int(os.environ["PORT"]), print=None)


if __name__ == "__main__":
    main()
