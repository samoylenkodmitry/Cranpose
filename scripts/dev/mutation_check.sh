#!/bin/sh
# Proves that a test guards a fix. Applies one sed expression to a source
# file, runs the named library tests expecting at least one to fail, and
# restores the file whatever happens.
#
# usage: scripts/dev/mutation_check.sh <package> <test-filter> <file> <sed-expression>
#
# Exit 0 when the mutation is killed (the tests fail), 1 when it survives,
# 2 when the expression did not change the file.
set -eu

package=$1
filter=$2
file=$3
expression=$4

backup="$file.mutation-backup"
cp "$file" "$backup"
trap 'mv "$backup" "$file"' EXIT

sed -e "$expression" "$backup" > "$file"
if cmp -s "$file" "$backup"; then
    echo "mutation did not change $file: $expression"
    exit 2
fi

if cargo test -p "$package" --lib "$filter" > /dev/null 2>&1; then
    echo "MUTATION SURVIVED: $filter still passes with '$expression' in $file"
    exit 1
fi
echo "mutation killed: $filter fails with '$expression' in $file"
