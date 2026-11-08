"""基準Python版の実HTTPをローカルで記録する。AWSへは接続しない。

JUMEAUX_PYTHON_ROOT=<Python版> .venv/bin/python -B compat/gen/miroir.py
stdoutのJSONをcompat/fixtures/miroir.jsonへ保存する。
ZIPはコンテナの時刻・圧縮表現を除き、全エントリの名前とbytesを固定する。
"""
import contextlib
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
from urllib.parse import unquote, parse_qs
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = Path(os.environ["JUMEAUX_PYTHON_ROOT"])
sys.path.insert(0, str(PYTHON_ROOT))

# 環境・共有プロファイル・メタデータ経由の外部アクセスを遮断する。
for key in list(os.environ):
    if key.startswith("AWS_") or key.lower().endswith("_proxy"):
        del os.environ[key]
os.environ.update(AWS_ACCESS_KEY_ID="test", AWS_SECRET_ACCESS_KEY="test",
                  AWS_DEFAULT_REGION="ap-northeast-1", AWS_REGION="ap-northeast-1",
                  AWS_EC2_METADATA_DISABLED="true", NO_PROXY="*")

from jumeaux.addons.final.miroir import Executor
from jumeaux.models import Report, FinalAddOnPayload, FinalAddOnReference

STS_RESPONSE = b'''<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/"><AssumeRoleResult><Credentials><AccessKeyId>ASIAEXAMPLE0000000000</AccessKeyId><SecretAccessKey>test-secret</SecretAccessKey><SessionToken>test-session</SessionToken><Expiration>2099-01-01T00:00:00Z</Expiration></Credentials><AssumedRoleUser><AssumedRoleId>id:session</AssumedRoleId><Arn>arn:aws:sts::123456789012:assumed-role/test/session</Arn></AssumedRoleUser></AssumeRoleResult><ResponseMetadata><RequestId>test</RequestId></ResponseMetadata></AssumeRoleResponse>'''


class Receiver(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        if self.headers.get("X-Amz-Target"):
            item = json.loads(body)
            if "tags" in item["Item"]:
                item["Item"]["tags"]["SS"].sort()
            self.server.records.append({"service": "dynamodb", "body": item,
                                        "token": self.headers.get("X-Amz-Security-Token")})
            response = b"{}"
            content_type = "application/x-amz-json-1.0"
        else:
            self.server.records.append({"service": "sts", "body": parse_qs(body.decode())})
            response = STS_RESPONSE
            content_type = "text/xml"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def do_PUT(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        record = {"service": "s3", "path": unquote(self.path),
                  "content_type": self.headers.get("Content-Type"),
                  "cache_control": self.headers.get("Cache-Control"),
                  "token": self.headers.get("X-Amz-Security-Token")}
        if self.path.endswith(".zip"):
            with zipfile.ZipFile(io.BytesIO(body)) as archive:
                record["members"] = {name: archive.read(name).hex() for name in sorted(archive.namelist())}
        else:
            record["hex"] = body.hex()
        self.server.records.append(record)
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()


def main():
    base = json.loads((ROOT / "compat/python-baseline/simple/report.json").read_text())
    base.update(key="miroir-日本語-key", title="日本語のタイトル")
    # report内のパスは実行場所に依存させない。payloadの保存先だけを差し替える。
    base["summary"]["output"]["response_dir"] = "responses"
    files = {"one/a.json": b'{"one":1}\n'.hex(), "other/b.bin": bytes(range(256)).hex(),
             "one-props/a.json": b'{"status":200}\n'.hex(), "other-props/b.json": b'{}\n'.hex(),
             "summary.txt": "結果\n".encode().hex(), "nested/extra.txt": b"extra".hex()}
    cases = []
    with tempfile.TemporaryDirectory() as tmp:
        os.environ.update(AWS_CONFIG_FILE=tmp + "/config", AWS_SHARED_CREDENTIALS_FILE=tmp + "/credentials")
        server = ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
        server.records = []
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        endpoint = f"http://127.0.0.1:{server.server_port}"
        os.environ["AWS_ENDPOINT_URL_STS"] = endpoint
        try:
            variants = [
                ("defaults", {}, {}),
                ("all-options", {"prefix": "日本語/prefix/", "cache_max_age": 3600,
                                 "checklist": "確認\n一覧", "assumed_role_arn": "arn:aws:iam::123456789012:role/test"},
                 {"description": "説明", "retry_hash": "previous"}),
                ("no-zip-empty-prefix", {"prefix": "", "cache_max_age": -1, "with_zip": False}, {}),
                ("utf16-zip", {}, {}),
                ("empty-send", {}, {}),
                ("skip-empty", {"when": ["not_empty"]}, {}),
                ("skip-same", {"when": ["has_different"]}, {}),
                ("both-send", {"when": ["not_empty", "has_different"], "with_zip": False}, {}),
                ("null-defaults", {"with_zip": None, "cache_max_age": None, "when": None}, {}),
            ]
            for name, options, updates in variants:
                data = copy.deepcopy(base)
                data.update(updates)
                if name == "all-options":
                    data["summary"]["tags"] = ["β", "alpha", "β"]
                    data["summary"]["status"] = {"same": 2, "different": 3, "failure": 4}
                    data["summary"]["time"]["elapsed_sec"] = 17
                if name in ("empty-send", "skip-empty"):
                    data["trials"] = []
                if name == "skip-same":
                    data["summary"]["status"]["different"] = 0
                if name == "utf16-zip":
                    data["summary"]["output"]["encoding"] = "utf-16"
                report = Report.from_dict(data)
                output = copy.deepcopy(data["summary"]["output"])
                output["response_dir"] = tmp + "/" + name
                dest = Path(output["response_dir"]) / data["key"]
                for directory in ("one", "other", "one-props", "other-props", "empty"):
                    (dest / directory).mkdir(parents=True)
                for path, body in files.items():
                    target = dest / path
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(bytes.fromhex(body))
                config = {"table": "test-table", "bucket": "test-bucket",
                          "endpoints": {"s3": endpoint, "dynamodb": endpoint}, **options}
                server.records = []
                with contextlib.redirect_stdout(io.StringIO()):
                    Executor(config).exec(FinalAddOnPayload.from_dict({"report": report, "output_summary": output}),
                                          FinalAddOnReference.from_dict({"notifiers": None}))
                config["endpoints"] = {"s3": "http://127.0.0.1:PORT", "dynamodb": "http://127.0.0.1:PORT"}
                cases.append({"name": name, "config": config, "report": report.to_dict(), "files": files,
                              "expected": server.records,
                              "local_report_hex": (dest / "report.json").read_bytes().hex() if (dest / "report.json").exists() else None})
        finally:
            server.shutdown()
            server.server_close()
            worker.join()
    print(json.dumps(cases, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
