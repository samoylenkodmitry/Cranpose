#!/usr/bin/env bash
set -euo pipefail
site="${1:?pass packaged site}"
output="${2:?pass output directory}"
driver_port="${3:?pass WebDriver port}"
server_port="${4:?pass HTTP port}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$output"
geckodriver --port "$driver_port" >"$output/driver.log" 2>&1 &
driver_pid=$!
python3 -m http.server "$server_port" --bind 127.0.0.1 --directory "$site" >"$output/server.log" 2>&1 &
server_pid=$!
trap 'kill "$driver_pid" "$server_pid" 2>/dev/null || true' EXIT
python3 "$script_dir/web_ime_robot.py" --endpoint "http://127.0.0.1:$driver_port" --browser firefox --url "http://127.0.0.1:$server_port" --output "$output/result"
