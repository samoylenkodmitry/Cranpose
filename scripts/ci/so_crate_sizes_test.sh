#!/usr/bin/env bash
set -euo pipefail

# so_crate_sizes.py attributes a native library's symbols to crates for the
# nightly size report. Every mangling shape it reads is pinned here, through
# a stand-in llvm-nm that prints fixed symbol lines, an llvm-strip that
# writes a file of known size and an llvm-size that lists its sections.

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/so_crate_sizes.py"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
failures=0

mkdir "$workdir/bin" "$workdir/empty-bin"
cat > "$workdir/bin/llvm-nm" <<'NM'
#!/usr/bin/env bash
cat <<'SYMBOLS'
0000000000001000 0000000000004000 T cranpose_ui::layout::measure::h0123456789abcdef
0000000000001400 0000000000002000 t _$LT$cranpose_ui..Box$u20$as$u20$core..fmt..Debug$GT$::fmt::h0123456789abcdef
0000000000001600 0000000000001000 T <naga[1a2b3c]::Module as core::default::Default>::default
0000000000001700 0000000000001000 t core::ptr::drop_in_place<cranpose_ui::Box>
0000000000001800 0000000000000800 t <[u8] as core::fmt::Debug>::fmt
0000000000001880 0000000000000800 T memcpy
0000000000001900 0000000000001000 R naga::back::TABLE
0000000000002000 0000000000100000 B cranpose_ui::STATE
SYMBOLS
NM
# llvm-strip --strip-all -o OUT IN: a stripped library of 48 KB.
cat > "$workdir/bin/llvm-strip" <<'STRIP'
#!/usr/bin/env bash
head -c 49152 /dev/zero > "$3"
STRIP
# Sections under 1% of the 48 KB file are left out of the report.
cat > "$workdir/bin/llvm-size" <<'SIZE'
#!/usr/bin/env bash
cat <<'SECTIONS'
stripped.so  :
section         size      addr
.rodata        12288    4096
.text          32768   16384
.comment         100       0
Total          45156
SECTIONS
SIZE
printf '#!/usr/bin/env bash\n' > "$workdir/empty-bin/llvm-nm"
chmod +x "$workdir/bin/llvm-nm" "$workdir/bin/llvm-strip" "$workdir/bin/llvm-size" "$workdir/empty-bin/llvm-nm"
: > "$workdir/lib.so"

top="$(python3 "$subject" "$workdir/lib.so" --llvm-bin "$workdir/bin" --top 2)"
all="$(python3 "$subject" "$workdir/lib.so" --llvm-bin "$workdir/bin")"

expect() {
    local label="$1" report="$2" pattern="$3"
    if grep -qF -- "$pattern" <<<"$report"; then
        echo "ok: $label"
    else
        echo "FAIL: $label: no line with '$pattern' in:"
        echo "$report"
        failures=$((failures + 1))
    fi
}

# Of 40 KB: cranpose_ui 16 + 8 (the legacy-escaped impl), naga 4 + 4, core
# 4 + 2 and memcpy 2.
expect "a legacy-mangled impl counts toward its self type's crate" "$top" "| cranpose_ui | 24 | 60.0% |"
expect "a v0 path and read-only data count toward their crate" "$top" "| naga | 8 | 20.0% |"
expect "the crates past the top are summed" "$top" "| 2 more crates | 8 | 20.0% |"
expect "zero-initialised data takes no file space" "$top" "Named code and data symbols: 40 KB."
expect "the stripped file is listed by its large sections, largest first" "$top" "Stripped, as an APK carries it: **48 KB** (\`.text\` 32, \`.rodata\` 12 KB)."
expect "generic and slice impls are core's" "$all" "| core | 6 | 15.0% |"
expect "a C symbol has no crate" "$all" "| (no Rust path) | 2 | 5.0% |"

if python3 "$subject" "$workdir/lib.so" --llvm-bin "$workdir/empty-bin" >/dev/null 2>&1; then
    echo "FAIL: a stripped library is reported as empty instead of refused"
    failures=$((failures + 1))
else
    echo "ok: a library with no symbols is refused"
fi

if ((failures > 0)); then
    echo "so_crate_sizes: $failures check(s) failed"
    exit 1
fi
echo "so_crate_sizes: all checks passed"
