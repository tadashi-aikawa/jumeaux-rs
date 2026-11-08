#!/usr/bin/env bash
# Pythonを使わず、配布バイナリだけでテンプレートを実行する。
set -eu
binary=$1
output=$2
shift 2
if [ "${JUMEAUX_REQUIRE_NO_PYTHON:-0}" = 1 ]; then
    if command -v python >/dev/null || command -v python3 >/dev/null; then
        echo 'Python must not be installed in the acceptance container' >&2
        exit 1
    fi
fi
if [ "$#" -eq 0 ]; then
    set -- all_same encoding_hell force_json html ignore ignore_order judge_response_headers notifier path_custom path_empty post query_custom request_headers root_array simple xml
fi
mkdir -p "$output"
output=$(cd "$output" && pwd)
server_pid=
cleanup() {
    if [ -n "$server_pid" ]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
        server_pid=
    fi
}
trap cleanup EXIT
export TZ=UTC NO_PROXY=localhost,127.0.0.1
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy SLACK_INCOMING_WEBHOOKS_URL
for template in "$@"; do
    mkdir "$output/$template"
    cd "$output/$template"
    "$binary" init "$template" > init.stdout 2> init.stderr
    # Last-ModifiedもOS間で比較できるよう、同梱APIファイルのmtimeを固定する。
    find api -type f -exec touch -t 200001010000.00 {} +
    : > server.stderr
    "$binary" server > server.stdout 2> server.stderr &
    server_pid=$!
    ready=false
    for attempt in {1..100}; do
        kill -0 "$server_pid"
        if IFS= read -r line < server.stderr; then
            case "$line" in
                'Serving HTTP on'*) ready=true; break ;;
            esac
        fi
        sleep 0.05
    done
    [ "$ready" = true ]
    "$binary" run requests > run.stdout 2> run.stderr
    cp responses/latest/report.json run-report.json
    "$binary" retry run-report.json > retry.stdout 2> retry.stderr
    cp responses/latest/report.json retry-report.json
    cleanup
    echo "PASS $template init/server/run/retry"
done
