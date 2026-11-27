#!/usr/bin/env bash
# The Mac reaches the stick over Wi-Fi. samarch-1 holds its authorized ADB key.
set -euo pipefail
: "${MI_TV_ENDPOINT:?set MI_TV_ENDPOINT to the TV host and port}"
relay=samarch-1
socket_dir=$(mktemp -d /tmp/cranpose-tv.XXXXXXXX)
control="$socket_dir/ssh"
export ADB_SERVER_SOCKET="localfilesystem:$socket_dir/adb"
cleanup() {
    if [[ -n ${serial:-} ]]; then adb disconnect "$serial" >/dev/null || true; fi
    ssh -S "$control" -O exit "$relay" >/dev/null 2>&1 || true
    rm -f "$socket_dir/adb" "$control"
    rmdir "$socket_dir"
}
trap cleanup EXIT
ssh -o BatchMode=yes -o ConnectTimeout=10 "$relay" /home/s/develop/sdk/platform-tools/adb start-server
ssh -fNM -o BatchMode=yes -o ExitOnForwardFailure=yes -S "$control" \
    -L "$socket_dir/adb:127.0.0.1:5037" "$relay"
port=$(ssh -S "$control" -O forward -R "127.0.0.1:0:$MI_TV_ENDPOINT" "$relay")
serial="127.0.0.1:$port"
adb connect "$serial"
model=$(adb -s "$serial" shell getprop ro.product.model)
if [[ $model != MiTV-AYFR0 ]]; then
    echo "Expected Mi TV Stick MiTV-AYFR0, found $model" >&2
    exit 1
fi
export ANDROID_DEVICE_LOCK_ID="$MI_TV_ENDPOINT"
"$@" "$serial"
