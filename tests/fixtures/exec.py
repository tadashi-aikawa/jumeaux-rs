import json
import os
import signal
import sys
import time

message = json.load(sys.stdin)
assert message["protocol_version"] == 1
action = message["options"].get("action", "echo")
if "marker" in message["options"]:
    with open(message["options"]["marker"], "a") as marker:
        marker.write(message["layer"] + " " + message["mode"] + "\n")
if message["options"].get("hang_mode") == message["mode"]:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    print("ready", file=sys.stderr, flush=True)
    time.sleep(30)
if action == "stderr":
    os.write(2, b"x" * 100000 + "終端\n".encode())
if action == "exit":
    print("diagnostic", file=sys.stderr)
    sys.exit(7)
if action == "timeout":
    time.sleep(10)
if action == "descendant":
    if os.fork() == 0:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        time.sleep(10)
    else:
        sys.exit(0)
if action == "invalid":
    print('{"duplicate":1,"duplicate":2}')
    sys.exit(0)
if action == "output":
    print(message["options"]["text"])
    sys.exit(0)
if message["mode"] == "validate":
    result = {"valid": action != "invalid_validate"}
elif message["mode"] == "json_transformer":
    if message["options"].get("expect_null_encoding"):
        assert message["payload"]["encoding"] is None
    result = message["options"].get("text", "transformed")
elif action == "environment":
    result = {"cwd": os.getcwd(), "env": dict(os.environ), "args": sys.argv[1:]}
elif message["layer"] == "log2reqs":
    result = [{"path": "/fixture"}]
else:
    result = message["payload"]
print(json.dumps(result, allow_nan=False))
