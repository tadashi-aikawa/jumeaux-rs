"""Python版のserverハンドラを継承し、実送信bytesを記録する。"""

import argparse
import base64
import errno
import io
import json
import socketserver
import socket
import sys
import threading
import time
from collections import Counter
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from jumeaux.commands.server.main import MyServerHandler, ReuseAddressTCPServer


class ExclusiveTCPServer(socketserver.ThreadingMixIn, ReuseAddressTCPServer):
    allow_reuse_address = False
    daemon_threads = True


def bind_server(address, handler):
    # allow_reuse_address=Falseでは終了直後のTIME_WAITもbindを妨げる。
    # LISTEN中は即時失敗し、接続不能な場合だけ期限付きで再試行する。
    deadline = time.monotonic() + 65
    while True:
        try:
            return ExclusiveTCPServer(address, handler)
        except OSError as error:
            if error.errno != errno.EADDRINUSE or time.monotonic() >= deadline:
                raise
            with socket.socket() as client:
                client.settimeout(0.3)
                if client.connect_ex(address) == 0:
                    raise RuntimeError(f"receiver port already in use: {address}") from error
            time.sleep(0.5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("workspace", type=Path)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    rules = json.loads(Path(__file__).with_name("comparison.json").read_text())
    settings = rules["server"]
    counts = Counter()
    lock = threading.Lock()

    def record(handler, filename, body):
        # ヘッダの綴り・重複・本文bytesも原記録に残す。
        item = {
            "method": handler.command,
            "url": "http://" + handler.headers["Host"] + handler.path,
            "headers": list(handler.headers.items()),
            "body_base64": base64.b64encode(body).decode("ascii"),
        }
        identity = json.dumps(item, sort_keys=True)
        with lock:
            counts[(filename, identity)] += 1
            item["occurrence"] = counts[(filename, identity)]
            with (workspace / filename).open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(item, ensure_ascii=False) + "\n")

    class API(MyServerHandler):
        protocol_version = "HTTP/1.1"

        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(workspace), **kwargs)

        def version_string(self):
            return settings["server_version"]

        def date_time_string(self, timestamp=None):
            return settings["http_date"]

        def send_header(self, keyword, value):
            if keyword.lower() == "last-modified":
                side = "one" if self.path.startswith("/api/one/") else "other"
                value = settings["last_modified"][side]
            super().send_header(keyword, value)

        def log_message(self, *args):
            pass

        def do_GET(self):
            record(self, "api.jsonl", b"")
            super().do_GET()

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            record(self, "api.jsonl", body)
            original = self.rfile
            self.rfile = io.BytesIO(body)
            try:
                super().do_POST()
            finally:
                self.rfile = original

    class Webhook(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            record(self, "notifications.jsonl", body)
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"ok")

    with bind_server(("127.0.0.1", settings["api_port"]), API) as api:
        with bind_server(("127.0.0.1", settings["notification_port"]), Webhook) as webhook:
            for server in (api, webhook):
                threading.Thread(target=server.serve_forever, daemon=True).start()
            (workspace / "receiver.ready").touch()
            for line in sys.stdin:
                workspace = Path(line.strip()).resolve()
                counts.clear()
                for name in ("api.jsonl", "notifications.jsonl"):
                    (workspace / name).write_text("")
                (workspace / "receiver.ready").touch()


if __name__ == "__main__":
    main()
