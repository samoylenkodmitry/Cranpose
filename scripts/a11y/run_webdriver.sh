#!/usr/bin/env bash
# Serves a packaged web demo and runs the shared keyboard checks of
# web_ime_robot.py in Firefox or Safari through the browser's WebDriver.
# Usage: run_webdriver.sh <firefox|safari> <site> <output>
# Both ports are free ones, so runs on a shared machine do not collide.
set -euo pipefail
browser="${1:?pass firefox or safari}"
site="${2:?pass packaged site}"
output="${3:?pass output directory}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
free_port() {
    python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}
driver_port="$(free_port)"
server_port="$(free_port)"
case "$browser" in
    firefox) driver=(geckodriver --port "$driver_port") ;;
    safari) driver=(safaridriver --port "$driver_port") ;;
    *) echo "unknown browser '$browser': pass firefox or safari" >&2; exit 64 ;;
esac
mkdir -p "$output"
"${driver[@]}" >"$output/driver.log" 2>&1 &
driver_pid=$!
python3 -m http.server "$server_port" --bind 127.0.0.1 --directory "$site" >"$output/server.log" 2>&1 &
server_pid=$!
trap 'kill "$driver_pid" "$server_pid" 2>/dev/null || true' EXIT
python3 "$script_dir/web_ime_robot.py" --endpoint "http://127.0.0.1:$driver_port" --browser "$browser" --url "http://127.0.0.1:$server_port" --output "$output/result"
