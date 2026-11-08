"""requestsの末尾へ1件追加する。Python 3標準ライブラリのみ。"""
import json
import sys


def main(message):
    if type(message.get("protocol_version")) is not int or message["protocol_version"] != 1:
        raise ValueError("unsupported protocol_version")
    if message.get("layer") != "reqs2reqs":
        raise ValueError("expected layer reqs2reqs")
    options = message.get("options")
    if not isinstance(options, dict) or set(options) != {"path"} or not isinstance(options["path"], str):
        raise ValueError("options requires path: string")
    if message.get("mode") == "validate":
        return {"valid": True}
    if message.get("mode") != "payload":
        raise ValueError("unsupported mode")
    payload = message["payload"]
    payload["requests"].append({
        "name": None, "method": "GET", "path": options["path"], "qs": {},
        "raw": None, "form": None, "json": None, "headers": {}, "url_encoding": "utf-8",
    })
    return payload


if __name__ == "__main__":
    try:
        result = main(json.loads(sys.stdin.buffer.read().decode("utf-8")))
        sys.stdout.buffer.write((json.dumps(result, allow_nan=False) + "\n").encode("utf-8"))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
