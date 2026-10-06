#!/usr/bin/env bash
set -euo pipefail

usage="usage: scripts/sprt.sh <base-ref> [extra fastchess options]

Builds dumbfish from <base-ref> and from the working tree, then plays them against each other
with fastchess until the SPRT accepts H0 or H1. Games go to target/sprt/sprt.pgn.

Environment (defaults):
  TC=8+0.08  CONCURRENCY=\$(nproc)  ELO0=0  ELO1=5
  BOOK=target/sprt/8moves_v3.pgn (downloaded on first use)
Non-regression test (simplifications): ELO0=-5 ELO1=0"

[[ $# -ge 1 && $1 != -h && $1 != --help ]] || { echo "$usage" >&2; exit 1; }
base_ref=$1
shift

root=$(git rev-parse --show-toplevel)
work=$root/target/sprt
mkdir -p "$work"

book=${BOOK:-$work/8moves_v3.pgn}
if [[ ! -f $book ]]; then
    curl -fsSL -o "$work/8moves_v3.pgn.zip" https://github.com/official-stockfish/books/raw/master/8moves_v3.pgn.zip
    unzip -o -q "$work/8moves_v3.pgn.zip" -d "$work"
fi

cargo build --release -p dumbfish --manifest-path "$root/Cargo.toml"
cp -f "$root/target/release/dumbfish" "$work/dev"

base_src=$work/base-src
git -C "$root" worktree remove --force "$base_src" 2>/dev/null || true
git -C "$root" worktree add --detach --quiet "$base_src" "$base_ref"
trap 'git -C "$root" worktree remove --force "$base_src"' EXIT
cargo build --release -p dumbfish --manifest-path "$base_src/Cargo.toml" --target-dir "$work/base-target"
cp -f "$work/base-target/release/dumbfish" "$work/base"

echo "dev:  working tree ($(git -C "$root" rev-parse --short HEAD)$(git -C "$root" diff --quiet HEAD || echo ', dirty'))"
echo "base: $base_ref ($(git -C "$root" rev-parse --short "$base_ref"))"

cd "$work"
fastchess \
    -engine cmd=./dev name=dev \
    -engine cmd=./base name=base \
    -each tc="${TC:-8+0.08}" \
    -openings file="$book" format=pgn order=random \
    -repeat -games 2 -rounds 50000 \
    -concurrency "${CONCURRENCY:-$(nproc)}" \
    -draw movenumber=40 movecount=8 score=10 \
    -resign movecount=3 score=600 \
    -sprt elo0="${ELO0:-0}" elo1="${ELO1:-5}" alpha=0.05 beta=0.05 \
    -pgnout file=sprt.pgn append=false \
    "$@"
