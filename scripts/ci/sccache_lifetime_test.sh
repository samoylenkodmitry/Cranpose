#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$script_dir/../dev_build_common.sh"
scratch="$(mktemp -d)"

cleanup() {
    rm -- "$scratch/sccache" "$scratch/start" "$scratch/compile"
    rmdir -- "$scratch"
}
trap cleanup EXIT

cat > "$scratch/sccache" <<'SH'
#!/usr/bin/env bash
case "$1" in
    --start-server) destination=start ;;
    *) destination=compile ;;
esac
printf '%s\n' "${RUNNER_TRACKING_ID-absent}" > "$SCCACHE_TEST_OUTPUT/$destination"
SH
chmod +x "$scratch/sccache"
export SCCACHE_TEST_OUTPUT="$scratch"
export RUSTC_WRAPPER="$scratch/sccache"
export RUNNER_TRACKING_ID=cranpose-test-job

enable_local_sccache
"$RUSTC_WRAPPER" rustc
if [[ "$(cat "$scratch/start")" != absent ]]; then
    echo "FAIL: the shared daemon inherits a job's cleanup identity" >&2
    exit 1
fi
if [[ "$(cat "$scratch/compile")" != cranpose-test-job ]]; then
    echo "FAIL: compilation escaped its job's cleanup identity" >&2
    exit 1
fi
echo "sccache lifetime: shared daemon is independent; compilation stays tracked"
