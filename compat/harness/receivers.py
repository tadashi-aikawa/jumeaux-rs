"""Python版のserverハンドラを継承し、実送信bytesを記録する。"""

import argparse
import base64
import io
import json
import signal
import threading
from collections import Counter
from http.server import BaseHTTPRequestHandler
from pathlib import Path

from jumeaux.commands.server.main import MyServerHandler, ReuseAddressTCPServer


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
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(workspace), **kwargs)

        def version_string(self):
            return settings["server_version"]

        def date_time_string(self, timestamp=None):
            return settings["http_date"]

        def log_message(self, *args):
            pass

        def do_GET(self):
            record(self, "api.jsonl", b"")
            super().do_GET()

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            record(self, "api.jsonl", body)
            self.rfile = io.BytesIO(body)
            super().do_POST()

    class Webhook(BaseHTTPRequestHandler):
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

    for name in ("api.jsonl", "notifications.jsonl"):
        (workspace / name).write_text("")
    with ReuseAddressTCPServer(("127.0.0.1", settings["api_port"]), API) as api:
        with ReuseAddressTCPServer(("127.0.0.1", settings["notification_port"]), Webhook) as webhook:
            for server in (api, webhook):
                threading.Thread(target=server.serve_forever, daemon=True).start()
            (workspace / "receiver.ready").touch()
            signal.pause()


if __name__ == "__main__":
    main()
