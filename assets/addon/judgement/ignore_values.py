"""unknown差分の値を調べ、指定文字列を含む差分をignore_valuesへ移す。"""
import json
import sys


def segments(path):
    if not isinstance(path, str) or not path.startswith("root"):
        raise ValueError("invalid diff path")
    index = 4
    result = []
    while index < len(path):
        if path[index] != "<":
            raise ValueError("invalid diff path")
        index += 1
        if index >= len(path):
            raise ValueError("invalid diff path")
        if path[index] in "\"'":
            quote = path[index]
            index += 1
            key = []
            while index < len(path) and path[index] != quote:
                char = path[index]
                index += 1
                if char == "\\":
                    if index >= len(path):
                        raise ValueError("invalid diff path escape")
                    escape = path[index]
                    index += 1
                    simple = {"\\": "\\", "'": "'", '"': '"', "n": "\n", "r": "\r", "t": "\t", "b": "\b", "f": "\f"}
                    if escape in simple:
                        char = simple[escape]
                    elif escape in "xuU":
                        count = {"x": 2, "u": 4, "U": 8}[escape]
                        digits = path[index:index + count]
                        if len(digits) != count or any(c not in "0123456789abcdefABCDEF" for c in digits):
                            raise ValueError("invalid diff path escape")
                        char = chr(int(digits, 16))
                        index += count
                    else:
                        raise ValueError("invalid diff path escape")
                key.append(char)
            if index >= len(path):
                raise ValueError("unterminated diff path key")
            index += 1
            result.append("".join(key))
        else:
            start = index
            while index < len(path) and path[index] in "0123456789":
                index += 1
            if index == start:
                raise ValueError("invalid diff path index")
            result.append(int(path[start:index]))
        if index >= len(path) or path[index] != ">":
            raise ValueError("invalid diff path")
        index += 1
    return result


def lookup(value, parts):
    for part in parts:
        if isinstance(part, str) and isinstance(value, dict):
            value = value.get(part)
        elif isinstance(part, int) and isinstance(value, list) and part < len(value):
            value = value[part]
        else:
            return None
    return value


def main(message):
    if type(message.get("protocol_version")) is not int or message["protocol_version"] != 1:
        raise ValueError("unsupported protocol_version")
    if message.get("layer") != "judgement":
        raise ValueError("expected layer judgement")
    options = message.get("options")
    if not isinstance(options, dict) or set(options) != {"values"} or not isinstance(options["values"], list) or not all(isinstance(v, str) for v in options["values"]):
        raise ValueError("options requires values: string[]")
    if message.get("mode") == "validate":
        return {"valid": True}
    if message.get("mode") != "payload":
        raise ValueError("unsupported mode")
    payload = message["payload"]
    diffs = payload.get("diffs_by_cognition")
    if not options["values"] or payload["regard_as_same_body"] or not diffs or "unknown" not in diffs:
        return payload
    reference = message["reference"]
    for kind in ("added", "changed", "removed"):
        remaining = []
        for path in diffs["unknown"][kind]:
            parts = segments(path)
            one = str(lookup(reference["dict_one"], parts))
            other = str(lookup(reference["dict_other"], parts))
            if any(v in one or v in other for v in options["values"]):
                diffs.setdefault("ignore_values", {"added": [], "changed": [], "removed": []})[kind].append(path)
            else:
                remaining.append(path)
        diffs["unknown"][kind] = remaining
    if not any(diffs["unknown"].values()):
        del diffs["unknown"]
        payload["regard_as_same_body"] = True
    return payload


if __name__ == "__main__":
    try:
        result = main(json.loads(sys.stdin.buffer.read().decode("utf-8")))
        sys.stdout.buffer.write((json.dumps(result, allow_nan=False) + "\n").encode("utf-8"))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
