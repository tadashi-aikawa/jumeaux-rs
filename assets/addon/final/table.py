"""指定列の表をstderrへ出し、payloadをそのまま返す。"""
import json
import sys


def main(message):
    if type(message.get("protocol_version")) is not int or message["protocol_version"] != 1:
        raise ValueError("unsupported protocol_version")
    if message.get("layer") != "final":
        raise ValueError("expected layer final")
    options = message.get("options")
    if not isinstance(options, dict) or set(options) != {"columns"} or not isinstance(options["columns"], list) or not all(isinstance(c, str) and c in {"seq", "name", "path", "status"} for c in options["columns"]):
        raise ValueError("options requires columns: seq/name/path/status[]")
    if message.get("mode") == "validate":
        return {"valid": True}
    if message.get("mode") != "payload":
        raise ValueError("unsupported mode")
    columns = options["columns"]
    print("\t".join(columns), file=sys.stderr)
    for trial in message["payload"]["report"]["trials"]:
        cells = [str(trial[column]).replace("\r", "\\r").replace("\n", "\\n").replace("\t", "\\t") for column in columns]
        print("\t".join(cells), file=sys.stderr)
    return message["payload"]


if __name__ == "__main__":
    try:
        result = main(json.loads(sys.stdin.buffer.read().decode("utf-8")))
        sys.stdout.buffer.write((json.dumps(result, allow_nan=False) + "\n").encode("utf-8"))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
