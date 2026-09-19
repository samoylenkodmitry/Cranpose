#!/bin/sh
# Proves that a test guards a fix. Applies one sed expression to a source
# file, runs the named library tests expecting at least one to fail, and
# restores the file whatever happens.
#
# usage: scripts/dev/mutation_check.sh <package> <test-filter> <file> <sed-expression> [cargo-args]
#
# The optional cargo arguments go to `cargo test`, for a package whose tests
# need features: "--no-default-features --features desktop,renderer-wgpu".
#
# Exit 0 when the mutation is killed (the tests fail), 1 when it survives,
# 2 when the expression did not change the file.
set -eu

package=$1
filter=$2
file=$3
expression=$4
cargo_args=${5:-}

backup="$file.mutation-backup"
cp "$file" "$backup"
trap 'mv "$backup" "$file"' EXIT

sed -e "$expression" "$backup" > "$file"
if cmp -s "$file" "$backup"; then
    echo "mutation did not change $file: $expression"
    exit 2
fi

# shellcheck disable=SC2086
if cargo test -p "$package" $cargo_args --lib "$filter" > /dev/null 2>&1; then
    echo "MUTATION SURVIVED: $filter still passes with '$expression' in $file"
    exit 1
fi
echo "mutation killed: $filter fails with '$expression' in $file"
