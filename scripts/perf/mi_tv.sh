#!/usr/bin/env bash
set -euo pipefail
output=$1
bench=benchmarks/compose-vs-cranpose
mkdir -p "$output/apks"
for app in cranpose compose; do
    project="$bench/$app-app"
    if [[ $app == cranpose ]]; then project="$project/android"; fi
    (cd "$project" && ./gradlew --no-daemon -q :app:assembleRelease -PcranposeReleaseAbis=armeabi-v7a)
    cp "$project/app/build/outputs/apk/release/app-release.apk" "$output/apks/$app.apk"
done
scripts/android/mi_tv.sh python3 "$bench/frameworks.py" --install "$output/apks" --output "$output" \
    --apps cranpose,compose --extra '--ei tier 8' --run 10 --require-all --full-run --main "$(git rev-parse HEAD)"
