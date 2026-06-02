#!/usr/bin/env bash
set -euo pipefail

readonly ndk_version="${1:-}"

if [[ -z "$ndk_version" ]]; then
  echo "usage: $0 <ndk-version>" >&2
  exit 64
fi

sdk_root=""
for candidate in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "/usr/local/lib/android/sdk" "$HOME/Android/Sdk"; do
  if [[ -n "$candidate" && -d "$candidate" ]]; then
    sdk_root="$candidate"
    break
  fi
done

if [[ -z "$sdk_root" ]]; then
  echo "Android SDK root was not found; set ANDROID_HOME or ANDROID_SDK_ROOT" >&2
  exit 1
fi

mapfile -t sdkmanager_candidates < <(
  find "$sdk_root/cmdline-tools" -path "*/bin/sdkmanager" -type f 2>/dev/null | sort -V
)

if [[ "${#sdkmanager_candidates[@]}" -eq 0 ]]; then
  echo "sdkmanager was not found under $sdk_root/cmdline-tools" >&2
  exit 1
fi

sdkmanager="${sdkmanager_candidates[-1]}"

set +o pipefail
yes | "$sdkmanager" --licenses
license_status="${PIPESTATUS[1]}"
set -o pipefail

if [[ "$license_status" -ne 0 ]]; then
  exit "$license_status"
fi

"$sdkmanager" "ndk;$ndk_version"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "ANDROID_HOME=$sdk_root"
    echo "ANDROID_SDK_ROOT=$sdk_root"
    echo "ANDROID_NDK_HOME=$sdk_root/ndk/$ndk_version"
  } >>"$GITHUB_ENV"
fi

if [[ -n "${GITHUB_PATH:-}" ]]; then
  {
    dirname "$sdkmanager"
    echo "$sdk_root/platform-tools"
  } >>"$GITHUB_PATH"
fi
