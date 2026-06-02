#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/dev_build_common.sh"

enable_local_tmpdir
enable_local_cargo_job_limit

usage() {
    cat <<'USAGE'
Usage:
  scripts/x11_compare_window.sh --baseline-root <path> --current-root <path> --label <name>

Options:
  --baseline-root <path>   Clean source tree for the baseline build.
  --current-root <path>    Source tree for the candidate build.
  --label <name>           Output filename prefix.
  --title <text>           Window title to capture [Cranpose Demo].
  --class <text>           X11 window class to capture instead of title.
  --package <name>         Cargo package to run [desktop-app].
  --bin <name>             Cargo binary to run [desktop-app].
  --example <name>         Cargo example to run instead of a binary.
  --profile <name>         Cargo profile to build [dev].
  --features <features>    Cargo features to enable for the build.
  --output-dir <path>      Capture output directory [target/x11-comparison].
  --baseline-target <path> Cargo target directory for baseline build [target/x11-comparison/baseline-target].
  --current-target <path>  Cargo target directory for candidate build [target/x11-comparison/current-target].
  --baseline-cargo-config <value>
                          Extra cargo --config value for the baseline build. Repeatable.
  --current-cargo-config <value>
                          Extra cargo --config value for the candidate build. Repeatable.
  --settle-ms <number>     Delay after window discovery before capture [1200].
  --window-wait-ms <number>
                          Maximum time to wait for the launched app window [8000].
  --launch-pointer-position <X,Y>
                          Move the pointer here before each launch to steer WM placement.
  --window-position <X,Y>  Move each captured window to this X11 screen position before capture.
  --window-size <WxH>      Resize each captured window before capture.
  --ignore-top-px <number> Also compare frames after cropping this many pixels from the top [0].
  --expected-size <WxH>   Fail unless baseline and current captures have this exact size.
  --max-normalized-rmse <number>
                          Fail when full-frame normalized RMSE exceeds this value.
  --max-cropped-normalized-rmse <number>
                          Fail when cropped normalized RMSE exceeds this value. Requires --ignore-top-px.
  --max-content-bounds-delta-px <number>
                          Fail when trimmed foreground bounds differ by more than this many pixels.
  --content-trim-fuzz <number>
                          ImageMagick fuzz percentage for foreground trimming [2].
  --expected-owner-command <name>
                          Fail unless the captured X11 window is owned by this process command.
  --isolated-app-home     Launch each app run with a clean variant-specific HOME and XDG dirs
                          under the output directory.
  --app-env <KEY=VALUE>   Environment variable passed to each launched app. Repeatable.
USAGE
}

baseline_root=
current_root=
label=
title="Cranpose Demo"
window_class=
package="desktop-app"
bin="desktop-app"
example=
cargo_profile="dev"
cargo_features=
output_dir="target/x11-comparison"
baseline_target=
current_target=
baseline_cargo_configs=()
current_cargo_configs=()
settle_ms=1200
window_wait_ms=8000
launch_pointer_position=
window_position=
window_size=
ignore_top_px=0
expected_size=
max_normalized_rmse=
max_cropped_normalized_rmse=
max_content_bounds_delta_px=
content_trim_fuzz=2
expected_owner_command=
isolated_app_home=0
app_env_vars=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --baseline-root)
            baseline_root="${2:?missing --baseline-root value}"
            shift 2
            ;;
        --current-root)
            current_root="${2:?missing --current-root value}"
            shift 2
            ;;
        --label)
            label="${2:?missing --label value}"
            shift 2
            ;;
        --title)
            title="${2:?missing --title value}"
            shift 2
            ;;
        --class)
            window_class="${2:?missing --class value}"
            shift 2
            ;;
        --package)
            package="${2:?missing --package value}"
            shift 2
            ;;
        --bin)
            bin="${2:?missing --bin value}"
            shift 2
            ;;
        --example)
            example="${2:?missing --example value}"
            shift 2
            ;;
        --profile)
            cargo_profile="${2:?missing --profile value}"
            shift 2
            ;;
        --features)
            cargo_features="${2:?missing --features value}"
            shift 2
            ;;
        --output-dir)
            output_dir="${2:?missing --output-dir value}"
            shift 2
            ;;
        --baseline-target)
            baseline_target="${2:?missing --baseline-target value}"
            shift 2
            ;;
        --current-target)
            current_target="${2:?missing --current-target value}"
            shift 2
            ;;
        --baseline-cargo-config)
            baseline_cargo_configs+=("${2:?missing --baseline-cargo-config value}")
            shift 2
            ;;
        --current-cargo-config)
            current_cargo_configs+=("${2:?missing --current-cargo-config value}")
            shift 2
            ;;
        --settle-ms)
            settle_ms="${2:?missing --settle-ms value}"
            shift 2
            ;;
        --window-wait-ms)
            window_wait_ms="${2:?missing --window-wait-ms value}"
            shift 2
            ;;
        --launch-pointer-position)
            launch_pointer_position="${2:?missing --launch-pointer-position value}"
            shift 2
            ;;
        --window-position)
            window_position="${2:?missing --window-position value}"
            shift 2
            ;;
        --window-size)
            window_size="${2:?missing --window-size value}"
            shift 2
            ;;
        --ignore-top-px)
            ignore_top_px="${2:?missing --ignore-top-px value}"
            shift 2
            ;;
        --expected-size)
            expected_size="${2:?missing --expected-size value}"
            shift 2
            ;;
        --max-normalized-rmse)
            max_normalized_rmse="${2:?missing --max-normalized-rmse value}"
            shift 2
            ;;
        --max-cropped-normalized-rmse)
            max_cropped_normalized_rmse="${2:?missing --max-cropped-normalized-rmse value}"
            shift 2
            ;;
        --max-content-bounds-delta-px)
            max_content_bounds_delta_px="${2:?missing --max-content-bounds-delta-px value}"
            shift 2
            ;;
        --content-trim-fuzz)
            content_trim_fuzz="${2:?missing --content-trim-fuzz value}"
            shift 2
            ;;
        --expected-owner-command)
            expected_owner_command="${2:?missing --expected-owner-command value}"
            shift 2
            ;;
        --isolated-app-home)
            isolated_app_home=1
            shift
            ;;
        --app-env)
            app_env_vars+=("${2:?missing --app-env value}")
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ -z "$baseline_root" ] || [ -z "$current_root" ] || [ -z "$label" ]; then
    usage >&2
    exit 2
fi

case "$settle_ms" in
    ''|*[!0-9]*)
        echo "--settle-ms must be a non-negative integer" >&2
        exit 2
        ;;
esac

case "$window_wait_ms" in
    ''|*[!0-9]*)
        echo "--window-wait-ms must be a positive integer" >&2
        exit 2
        ;;
esac
if [ "$window_wait_ms" -le 0 ]; then
    echo "--window-wait-ms must be a positive integer" >&2
    exit 2
fi

case "$ignore_top_px" in
    ''|*[!0-9]*)
        echo "--ignore-top-px must be a non-negative integer" >&2
        exit 2
        ;;
esac

validate_float() {
    local name="$1"
    local value="$2"
    if [ -z "$value" ]; then
        return
    fi
    if ! awk -v value="$value" 'BEGIN { exit(value ~ /^[0-9]+([.][0-9]+)?$/ ? 0 : 1) }'; then
        echo "$name must be a non-negative decimal number" >&2
        exit 2
    fi
}

validate_float "--max-normalized-rmse" "$max_normalized_rmse"
validate_float "--max-cropped-normalized-rmse" "$max_cropped_normalized_rmse"
validate_float "--content-trim-fuzz" "$content_trim_fuzz"

case "$cargo_profile" in
    ''|*[!A-Za-z0-9_-]*)
        echo "--profile must be a cargo profile name" >&2
        exit 2
        ;;
esac

if [ -n "$max_content_bounds_delta_px" ]; then
    case "$max_content_bounds_delta_px" in
        ''|*[!0-9]*)
            echo "--max-content-bounds-delta-px must be a non-negative integer" >&2
            exit 2
            ;;
    esac
fi

if [ -n "$expected_size" ]; then
    if ! awk -v value="$expected_size" 'BEGIN { exit(value ~ /^[1-9][0-9]*x[1-9][0-9]*$/ ? 0 : 1) }'; then
        echo "--expected-size must use WxH with positive integer dimensions" >&2
        exit 2
    fi
fi

if [ -n "$window_position" ]; then
    if ! awk -v value="$window_position" 'BEGIN { exit(value ~ /^-?[0-9]+,-?[0-9]+$/ ? 0 : 1) }'; then
        echo "--window-position must use X,Y integer coordinates" >&2
        exit 2
    fi
fi

if [ -n "$launch_pointer_position" ]; then
    if ! awk -v value="$launch_pointer_position" 'BEGIN { exit(value ~ /^-?[0-9]+,-?[0-9]+$/ ? 0 : 1) }'; then
        echo "--launch-pointer-position must use X,Y integer coordinates" >&2
        exit 2
    fi
fi

if [ -n "$window_size" ]; then
    if ! awk -v value="$window_size" 'BEGIN { exit(value ~ /^[1-9][0-9]*x[1-9][0-9]*$/ ? 0 : 1) }'; then
        echo "--window-size must use WxH with positive integer dimensions" >&2
        exit 2
    fi
fi

if [ -n "$max_cropped_normalized_rmse" ] && [ "$ignore_top_px" -le 0 ]; then
    echo "--max-cropped-normalized-rmse requires --ignore-top-px" >&2
    exit 2
fi

if [ -z "${DISPLAY:-}" ]; then
    echo "DISPLAY is not set; X11 capture cannot run" >&2
    exit 1
fi

for command_name in awk cargo ps xdotool xprop import magick; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "required command not found: $command_name" >&2
        exit 1
    fi
done

if ! command -v xwd >/dev/null 2>&1; then
    echo "required command not found: xwd" >&2
    exit 1
fi

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
baseline_target="${baseline_target:-$output_dir/baseline-target}"
current_target="${current_target:-$output_dir/current-target}"
mkdir -p "$baseline_target" "$current_target"

sanitize_label() {
    printf '%s' "$1" | tr -c 'A-Za-z0-9_.-' '_'
}

log_phase() {
    echo "[x11-compare] $*" >&2
}

window_ids_for_title() {
    if [ -n "$window_class" ]; then
        xdotool search --onlyvisible --class "$window_class" 2>/dev/null || true
        return
    fi

    local window_id
    while IFS= read -r window_id; do
        if [ -z "$window_id" ]; then
            continue
        fi
        if window_exact_title "$window_id"; then
            printf '%s\n' "$window_id"
        fi
    done < <(xdotool search --onlyvisible --name "$title" 2>/dev/null || true)
}

window_exact_title() {
    local window_id="$1"
    [ "$(xdotool getwindowname "$window_id" 2>/dev/null || true)" = "$title" ]
}

window_display_title() {
    local window_id="$1"
    xdotool getwindowname "$window_id" 2>/dev/null || true
}

window_selector_label() {
    if [ -n "$window_class" ]; then
        printf "class '%s'" "$window_class"
    else
        printf "title '%s'" "$title"
    fi
}

window_is_known() {
    local candidate="$1"
    shift
    for known in "$@"; do
        if [ "$candidate" = "$known" ]; then
            return 0
        fi
    done
    return 1
}

window_owner_pid() {
    local window_id="$1"
    xprop -id "$window_id" _NET_WM_PID 2>/dev/null \
        | awk -F'= ' '/_NET_WM_PID/ { print $2; found = 1 } END { if (!found) exit 1 }' \
        | awk '/^[0-9]+$/ { print $1; found = 1 } END { if (!found) exit 1 }'
}

process_command_name() {
    local pid="$1"
    ps -o comm= -p "$pid" 2>/dev/null | awk 'NF { print $1; found = 1 } END { if (!found) exit 1 }'
}

window_class_name() {
    local window_id="$1"
    xprop -id "$window_id" WM_CLASS 2>/dev/null \
        | sed -n 's/^WM_CLASS(STRING) = //p' \
        | tr -d '"'
}

assert_expected_owner_command() {
    local window_id="$1"
    if [ -z "$expected_owner_command" ]; then
        return
    fi

    local owner_pid owner_command
    owner_pid="$(window_owner_pid "$window_id")"
    owner_command="$(process_command_name "$owner_pid")"
    if [ "$owner_command" != "$expected_owner_command" ]; then
        echo "captured X11 window $window_id owner command '$owner_command' differs from expected '$expected_owner_command'" >&2
        echo "captured window class: $(window_class_name "$window_id" || true)" >&2
        exit 1
    fi
}

apply_capture_window_geometry() {
    local window_id="$1"

    if [ -n "$window_position" ]; then
        local x="${window_position%,*}"
        local y="${window_position#*,}"
        log_phase "applying capture window position ${x},${y}"
        xdotool windowmove "$window_id" "$x" "$y"
    fi

    if [ -n "$window_size" ]; then
        local width="${window_size%x*}"
        local height="${window_size#*x}"
        log_phase "applying capture window size ${width}x${height}"
        xdotool windowsize "$window_id" "$width" "$height"
    fi
}

apply_launch_pointer_position() {
    if [ -z "$launch_pointer_position" ]; then
        return
    fi

    local x="${launch_pointer_position%,*}"
    local y="${launch_pointer_position#*,}"
    log_phase "moving pointer to ${x},${y} before launch"
    xdotool mousemove "$x" "$y"
}

process_is_descendant_of() {
    local candidate="$1"
    local ancestor="$2"

    while [ -n "$candidate" ] && [ "$candidate" != "0" ]; do
        if [ "$candidate" = "$ancestor" ]; then
            return 0
        fi
        candidate="$(ps -o ppid= -p "$candidate" 2>/dev/null | awk '{ print $1 }')"
    done

    return 1
}

window_belongs_to_process_tree() {
    local window_id="$1"
    local root_pid="$2"
    local owner_pid

    if ! owner_pid="$(window_owner_pid "$window_id")"; then
        return 1
    fi

    process_is_descendant_of "$owner_pid" "$root_pid"
}

capture_window() {
    local window_id="$1"
    local output_png="$2"
    local xwd_path="${output_png%.png}.xwd"

    if import -silent -window "$window_id" "png:$output_png" && [ -s "$output_png" ]; then
        return 0
    fi

    if xwd -silent -id "$window_id" -out "$xwd_path" && [ -s "$xwd_path" ]; then
        if magick "$xwd_path" "$output_png" && [ -s "$output_png" ]; then
            mv "$xwd_path" "${xwd_path}.captured"
            return 0
        fi
    fi

    echo "failed to capture X11 window $window_id" >&2
    return 1
}

normalized_rmse() {
    awk '
        /\([0-9.]+\)/ {
            value = $0
            sub(/^[^(]*\(/, "", value)
            sub(/\).*/, "", value)
            print value
            found = 1
        }
        END {
            if (!found) exit 1
        }
    ' "$1"
}

assert_normalized_rmse_at_most() {
    local label="$1"
    local metric_path="$2"
    local threshold="$3"
    if [ -z "$threshold" ]; then
        return
    fi
    local value
    value="$(normalized_rmse "$metric_path")"
    if ! awk -v value="$value" -v threshold="$threshold" 'BEGIN { exit(value <= threshold ? 0 : 1) }'; then
        echo "$label normalized RMSE $value exceeds threshold $threshold" >&2
        exit 1
    fi
}

abs_delta() {
    local lhs="$1"
    local rhs="$2"
    local delta=$((lhs - rhs))
    if [ "$delta" -lt 0 ]; then
        delta=$((-delta))
    fi
    printf '%s\n' "$delta"
}

content_bounds_geometry() {
    local image_path="$1"
    local width="$2"
    local height="$3"
    local crop_top="$4"

    if [ "$crop_top" -gt 0 ]; then
        local cropped_height=$((height - crop_top))
        if [ "$cropped_height" -le 0 ]; then
            echo "--ignore-top-px must be smaller than capture height $height" >&2
            exit 1
        fi
        magick "$image_path" \
            -crop "${width}x${cropped_height}+0+${crop_top}" \
            +repage \
            -fuzz "${content_trim_fuzz}%" \
            -trim \
            -format '%wx%h%O' \
            info:
        return
    fi

    magick "$image_path" \
        -fuzz "${content_trim_fuzz}%" \
        -trim \
        -format '%wx%h%O' \
        info:
}

content_bounds_components() {
    local image_path="$1"
    local width="$2"
    local height="$3"
    local crop_top="$4"
    local geometry
    geometry="$(content_bounds_geometry "$image_path" "$width" "$height" "$crop_top")"
    if [[ ! "$geometry" =~ ^([0-9]+)x([0-9]+)\+([0-9]+)\+([0-9]+)$ ]]; then
        echo "failed to parse content bounds '$geometry' for $image_path" >&2
        exit 1
    fi
    printf '%s %s %s %s %s\n' \
        "${BASH_REMATCH[3]}" \
        "${BASH_REMATCH[4]}" \
        "${BASH_REMATCH[1]}" \
        "${BASH_REMATCH[2]}" \
        "$geometry"
}

assert_content_bounds_delta_at_most() {
    if [ -z "$max_content_bounds_delta_px" ]; then
        return
    fi

    local baseline_path="$1"
    local current_path="$2"
    local size="$3"
    local crop_top="$4"
    local width="${size%x*}"
    local height="${size#*x}"

    local baseline_x baseline_y baseline_width baseline_height baseline_geometry
    local current_x current_y current_width current_height current_geometry
    read -r baseline_x baseline_y baseline_width baseline_height baseline_geometry \
        < <(content_bounds_components "$baseline_path" "$width" "$height" "$crop_top")
    read -r current_x current_y current_width current_height current_geometry \
        < <(content_bounds_components "$current_path" "$width" "$height" "$crop_top")

    echo "baseline content bounds: $baseline_geometry"
    echo "current content bounds:  $current_geometry"

    local field expected actual delta
    for field in x y width height; do
        case "$field" in
            x)
                expected="$baseline_x"
                actual="$current_x"
                ;;
            y)
                expected="$baseline_y"
                actual="$current_y"
                ;;
            width)
                expected="$baseline_width"
                actual="$current_width"
                ;;
            height)
                expected="$baseline_height"
                actual="$current_height"
                ;;
        esac
        delta="$(abs_delta "$actual" "$expected")"
        if [ "$delta" -gt "$max_content_bounds_delta_px" ]; then
            echo "content bounds $field delta $delta exceeds threshold $max_content_bounds_delta_px" >&2
            exit 1
        fi
    done
}

run_and_capture() {
    local root="$1"
    local target_dir="$2"
    local variant="$3"
    local prefix="$output_dir/$(sanitize_label "$label")-$variant"
    local png="$prefix.png"
    local log="$prefix.log"
    local profile_dir="$cargo_profile"
    case "$cargo_profile" in
        dev)
            profile_dir="debug"
            ;;
        release)
            profile_dir="release"
            ;;
    esac

    local binary
    local cargo_build_args=(build --package "$package")
    if [ "$cargo_profile" != "dev" ]; then
        cargo_build_args+=(--profile "$cargo_profile")
    fi
    if [ -n "$cargo_features" ]; then
        cargo_build_args+=(--features "$cargo_features")
    fi
    if [ -n "$example" ]; then
        cargo_build_args+=(--example "$example")
        binary="$target_dir/$profile_dir/examples/$example"
    else
        cargo_build_args+=(--bin "$bin")
        binary="$target_dir/$profile_dir/$bin"
    fi

    if [ "$variant" = "baseline" ]; then
        for cargo_config in "${baseline_cargo_configs[@]}"; do
            cargo_build_args+=(--config "$cargo_config")
        done
    else
        for cargo_config in "${current_cargo_configs[@]}"; do
            cargo_build_args+=(--config "$cargo_config")
        done
    fi

    log_phase "$variant: building $package in $root"
    (
        cd "$root"
        env \
            TMPDIR="${TMPDIR:-$(ensure_local_temp_root)}" \
            CARGO_TARGET_DIR="$target_dir" \
            cargo "${cargo_build_args[@]}"
    ) > "$log" 2>&1
    log_phase "$variant: build finished"

    if [ ! -x "$binary" ] && [ -x "${binary}.exe" ]; then
        binary="${binary}.exe"
    fi
    if [ ! -x "$binary" ]; then
        echo "built binary was not found: $binary" >&2
        echo "see log: $log" >&2
        exit 1
    fi

    mapfile -t before_windows < <(window_ids_for_title)

    apply_launch_pointer_position
    log_phase "$variant: launching $binary"
    if [ "$isolated_app_home" = "1" ]; then
        local app_home="$output_dir/$(sanitize_label "$label")-$variant-home"
        local app_xauthority="${XAUTHORITY:-$HOME/.Xauthority}"
        mkdir -p \
            "$app_home" \
            "$app_home/.config" \
            "$app_home/.local/share" \
            "$app_home/.cache"
        log_phase "$variant: using isolated app HOME $app_home"
        env \
            HOME="$app_home" \
            XDG_CONFIG_HOME="$app_home/.config" \
            XDG_DATA_HOME="$app_home/.local/share" \
            XDG_CACHE_HOME="$app_home/.cache" \
            XAUTHORITY="$app_xauthority" \
            "${app_env_vars[@]}" \
            "$binary" >> "$log" 2>&1 &
    else
        if [ "${#app_env_vars[@]}" -gt 0 ]; then
            env "${app_env_vars[@]}" "$binary" >> "$log" 2>&1 &
        else
            "$binary" >> "$log" 2>&1 &
        fi
    fi
    local app_pid=$!

    local window_id=
    local window_wait_attempts=$(((window_wait_ms + 99) / 100))
    log_phase "$variant: waiting up to ${window_wait_ms}ms for $(window_selector_label)"
    for ((attempt = 0; attempt < window_wait_attempts; attempt++)); do
        mapfile -t current_windows < <(window_ids_for_title)
        for candidate in "${current_windows[@]}"; do
            if ! window_is_known "$candidate" "${before_windows[@]}"; then
                if window_belongs_to_process_tree "$candidate" "$app_pid"; then
                    window_id="$candidate"
                    break
                fi
            fi
        done
        if [ -n "$window_id" ]; then
            break
        fi
        sleep 0.1
    done

    if [ -z "$window_id" ]; then
        kill "$app_pid" 2>/dev/null || true
        wait "$app_pid" 2>/dev/null || true
        echo "window $(window_selector_label) was not created by $variant run" >&2
        echo "see log: $log" >&2
        exit 1
    fi

    log_phase "$variant: captured window id $window_id for pid $app_pid"
    assert_expected_owner_command "$window_id"
    log_phase "$variant: captured window owner $(window_owner_pid "$window_id") command $(process_command_name "$(window_owner_pid "$window_id")") title $(window_display_title "$window_id") class $(window_class_name "$window_id" || true)"
    apply_capture_window_geometry "$window_id"
    log_phase "$variant: waiting ${settle_ms}ms before capture"
    sleep "$(awk "BEGIN { printf \"%.3f\", $settle_ms / 1000 }")"
    log_phase "$variant: capturing window"
    capture_window "$window_id" "$png"

    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
    log_phase "$variant: capture saved to $png"

    printf '%s\n' "$png"
}

log_phase "starting baseline capture for $label"
baseline_png="$(run_and_capture "$baseline_root" "$baseline_target" baseline)"
log_phase "starting current capture for $label"
current_png="$(run_and_capture "$current_root" "$current_target" current)"
diff_png="$output_dir/$(sanitize_label "$label")-diff.png"
metric_file="$output_dir/$(sanitize_label "$label")-rmse.txt"

log_phase "comparing captures"
set +e
magick compare -metric RMSE "$baseline_png" "$current_png" "$diff_png" 2>"$metric_file"
compare_status=$?
set -e

baseline_size="$(magick identify -format '%wx%h' "$baseline_png")"
current_size="$(magick identify -format '%wx%h' "$current_png")"
metric="$(cat "$metric_file")"

echo "baseline: $baseline_png ($baseline_size)"
echo "current:  $current_png ($current_size)"
echo "diff:     $diff_png"
echo "rmse:     $metric"

if [ -n "$expected_size" ]; then
    if [ "$baseline_size" != "$expected_size" ]; then
        echo "baseline capture size $baseline_size differs from expected $expected_size" >&2
        exit 1
    fi
    if [ "$current_size" != "$expected_size" ]; then
        echo "current capture size $current_size differs from expected $expected_size" >&2
        exit 1
    fi
fi

if [ "$baseline_size" != "$current_size" ]; then
    echo "capture sizes differ" >&2
    exit 1
fi

assert_normalized_rmse_at_most "full-frame" "$metric_file" "$max_normalized_rmse"

assert_content_bounds_delta_at_most "$baseline_png" "$current_png" "$baseline_size" "$ignore_top_px"

if [ "$compare_status" -gt 1 ]; then
    echo "ImageMagick compare failed with status $compare_status" >&2
    exit "$compare_status"
fi

if [ "$ignore_top_px" -gt 0 ]; then
    width="${baseline_size%x*}"
    height="${baseline_size#*x}"
    cropped_height=$((height - ignore_top_px))
    if [ "$cropped_height" -le 0 ]; then
        echo "--ignore-top-px must be smaller than capture height $height" >&2
        exit 1
    fi

    cropped_baseline="$output_dir/$(sanitize_label "$label")-baseline-below-top.png"
    cropped_current="$output_dir/$(sanitize_label "$label")-current-below-top.png"
    cropped_diff="$output_dir/$(sanitize_label "$label")-diff-below-top.png"
    cropped_metric_file="$output_dir/$(sanitize_label "$label")-rmse-below-top.txt"

    magick "$baseline_png" -crop "${width}x${cropped_height}+0+${ignore_top_px}" +repage "$cropped_baseline"
    magick "$current_png" -crop "${width}x${cropped_height}+0+${ignore_top_px}" +repage "$cropped_current"

    set +e
    magick compare -metric RMSE "$cropped_baseline" "$cropped_current" "$cropped_diff" 2>"$cropped_metric_file"
    cropped_compare_status=$?
    set -e

    if [ "$cropped_compare_status" -gt 1 ]; then
        echo "ImageMagick cropped compare failed with status $cropped_compare_status" >&2
        exit "$cropped_compare_status"
    fi

    echo "cropped baseline: $cropped_baseline (${width}x${cropped_height})"
    echo "cropped current:  $cropped_current (${width}x${cropped_height})"
    echo "cropped diff:     $cropped_diff"
    echo "cropped rmse:     $(cat "$cropped_metric_file")"
    assert_normalized_rmse_at_most "cropped-frame" \
        "$cropped_metric_file" \
        "$max_cropped_normalized_rmse"
fi
