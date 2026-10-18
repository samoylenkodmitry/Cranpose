#!/bin/sh
# Builds build/perf-compare-fyne here, as `desktop.py` runs it: Fyne's release
# build, as `fyne package -release` makes it, with the thread checks of its
# fyne.Do migration off. The app runs every Fyne call on the main goroutine,
# and each check costs a stack parse on every Refresh.
set -eu
cd "$(dirname "$0")"
go build -tags release,migrated_fynedo -o build/perf-compare-fyne .
