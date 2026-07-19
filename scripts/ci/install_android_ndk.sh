#!/usr/bin/env bash
set -euo pipefail

readonly ndk_version="${1:-}"

if [[ -z "$ndk_version" ]]; then
  echo "usage: $0 <ndk-version>" >&2
  exit 64
fi

sdk_root=""
# Candidates cover GitHub-hosted Linux, Linux defaults, and the macOS
# default used by the self-hosted Mac release runners.
for candidate in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "/usr/local/lib/android/sdk" "$HOME/Android/Sdk" "$HOME/Library/Android/sdk"; do
  if [[ -n "$candidate" && -d "$candidate" ]]; then
    sdk_root="$candidate"
    break
  fi
done

if [[ -z "$sdk_root" ]]; then
  echo "Android SDK root was not found; set ANDROID_HOME or ANDROID_SDK_ROOT" >&2
  exit 1
fi

sdkmanager=""
while IFS= read -r candidate; do
  sdkmanager="$candidate"
done < <(find "$sdk_root/cmdline-tools" -path "*/bin/sdkmanager" -type f 2>/dev/null | sort)

if [[ -z "$sdkmanager" ]]; then
  echo "sdkmanager was not found under $sdk_root/cmdline-tools" >&2
  exit 1
fi

set +o pipefail
yes | "$sdkmanager" --licenses
license_status="${PIPESTATUS[1]}"
set -o pipefail

if [[ "$license_status" -ne 0 ]]; then
  exit "$license_status"
fi

# Install the NDK robustly: sdkmanager's download intermittently corrupts
# ("Error on ZipFile unknown archive") with no built-in retry, which fails the
# heavy Android build. Skip if already present, retry with a clean slate, then
# fall back to a direct download from Google.
ndk_dir="$sdk_root/ndk/$ndk_version"

ndk_installed() {
  [[ -f "$ndk_dir/source.properties" ]]
}

if ndk_installed; then
  echo "NDK $ndk_version already installed at $ndk_dir"
else
  installed=0
  for attempt in 1 2 3; do
    echo "Installing ndk;$ndk_version via sdkmanager (attempt $attempt)..."
    if "$sdkmanager" "ndk;$ndk_version" && ndk_installed; then
      installed=1
      break
    fi
    echo "sdkmanager NDK install failed; clearing partial downloads and retrying" >&2
    rm -rf "$ndk_dir" "$sdk_root/.temp" "$sdk_root/.downloadIntermediates" 2>/dev/null || true
    sleep 5
  done

  if [[ "$installed" -ne 1 ]]; then
    # Fallback: fetch the NDK zip directly from Google (retried), avoiding
    # sdkmanager's flaky archive handling entirely.
    major="${ndk_version%%.*}"
    url="https://dl.google.com/android/repository/android-ndk-r${major}-linux.zip"
    echo "Falling back to direct NDK download: $url" >&2
    tmp="$(mktemp -d)"
    for attempt in 1 2 3; do
      if curl -fL --retry 5 --retry-delay 5 -o "$tmp/ndk.zip" "$url" \
        && unzip -q -o "$tmp/ndk.zip" -d "$tmp"; then
        extracted="$(find "$tmp" -maxdepth 1 -type d -name 'android-ndk-*' | head -n1)"
        if [[ -n "$extracted" ]]; then
          mkdir -p "$sdk_root/ndk"
          rm -rf "$ndk_dir"
          mv "$extracted" "$ndk_dir"
        fi
      fi
      ndk_installed && break
      echo "direct NDK download attempt $attempt failed" >&2
      rm -f "$tmp/ndk.zip"
      sleep 5
    done
    rm -rf "$tmp"
  fi

  if ! ndk_installed; then
    echo "NDK $ndk_version install failed after all retries and fallback" >&2
    exit 1
  fi
fi

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
