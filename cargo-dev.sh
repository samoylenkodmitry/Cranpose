#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/dev_build_common.sh"

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

if [ -n "${RUSTC_WRAPPER:-}" ]; then
    echo "Using local rustc wrapper: $RUSTC_WRAPPER"
fi

if [ -n "${TMPDIR:-}" ]; then
    echo "Using local tmpdir: $TMPDIR"
fi

if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
    echo "Using local cargo build jobs: $CARGO_BUILD_JOBS"
fi

exec cargo "$@"
