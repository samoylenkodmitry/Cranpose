#!/usr/bin/env bash
# Runs a command beside an accessibility bus of its own: a dbus-daemon with
# at-spi2's configuration and the AT-SPI registry on it. Run it on a private
# session bus (dbus-run-session). at-spi-bus-launcher starts its bus through
# dbus-broker where the system has it, and dbus-broker asks for a systemd user
# manager that a private session bus does not have, so no bus ever starts.
# The app (AccessKit) and the robot (libatspi) both read AT_SPI_BUS_ADDRESS.
set -euo pipefail
config=/usr/share/defaults/at-spi2/accessibility.conf
registryd=""
for candidate in /usr/libexec/at-spi2-registryd /usr/lib/at-spi2-registryd \
    /usr/lib/at-spi2-core/at-spi2-registryd; do
    if [ -x "$candidate" ]; then
        registryd="$candidate"
        break
    fi
done
if [ -z "$registryd" ] || [ ! -f "$config" ]; then
    echo "linux_a11y_bus: at-spi2-core is not installed (no registryd or $config)" >&2
    exit 1
fi
{ read -r address; read -r bus_pid; } < <(dbus-daemon --config-file="$config" --fork \
    --print-address=1 --print-pid=1)
export AT_SPI_BUS_ADDRESS="$address"
"$registryd" --use-gnome-session &
registry_pid=$!
trap 'kill "$registry_pid" "$bus_pid" 2>/dev/null || true' EXIT
"$@"
