#!/usr/bin/env bash
set -euo pipefail

usage="usage: scripts/sprt.sh <base-ref> [<dev-ref>] [-- extra fastchess options]

Builds dumbfish from <base-ref> and from <dev-ref> (the working tree if omitted), then plays them
against each other with fastchess until the SPRT accepts H0 or H1. Games go to target/sprt/sprt.pgn.

Environment (defaults):
  TC=8+0.08  CONCURRENCY=\$(nproc)  ELO0=0  ELO1=5
  DEV_OPTIONS, BASE_OPTIONS: UCI options for one engine, e.g. \"Threads=2 Hash=64\"
  BOOK=target/sprt/8moves_v3.pgn (downloaded on first use)
Non-regression test (simplifications): ELO0=-5 ELO1=0"

[[ $# -ge 1 && $1 != -h && $1 != --help ]] || { echo "$usage" >&2; exit 1; }
base_ref=$1
shift
dev_ref=
if [[ $# -ge 1 && $1 != -- ]]; then
    dev_ref=$1
    shift
fi
[[ $# -ge 1 && $1 == -- ]] && shift

root=$(git rev-parse --show-toplevel)
work=$root/target/sprt
mkdir -p "$work"

book=${BOOK:-$work/8moves_v3.pgn}
if [[ ! -f $book ]]; then
    curl -fsSL -o "$work/8moves_v3.pgn.zip" https://github.com/official-stockfish/books/raw/master/8moves_v3.pgn.zip
    unzip -o -q "$work/8moves_v3.pgn.zip" -d "$work"
fi

worktrees=()
cleanup() {
    for tree in "${worktrees[@]}"; do
        git -C "$root" worktree remove --force "$tree"
    done
}
trap cleanup EXIT

build_ref() {
    local ref=$1 name=$2
    local src=$work/$name-src
    git -C "$root" worktree remove --force "$src" 2>/dev/null || true
    git -C "$root" worktree add --detach --quiet "$src" "$ref"
    worktrees+=("$src")
    cargo build --release -p dumbfish --manifest-path "$src/Cargo.toml" --target-dir "$work/$name-target"
    cp -f "$work/$name-target/release/dumbfish" "$work/$name"
    echo "$name: $ref ($(git -C "$root" rev-parse --short "$ref"))"
}

build_ref "$base_ref" base
if [[ -n $dev_ref ]]; then
    build_ref "$dev_ref" dev
else
    cargo build --release -p dumbfish --manifest-path "$root/Cargo.toml"
    cp -f "$root/target/release/dumbfish" "$work/dev"
    echo "dev: working tree ($(git -C "$root" rev-parse --short HEAD)$(git -C "$root" diff --quiet HEAD || echo ', dirty'))"
fi

options() {
    for option in $1; do
        printf '%s\0' "option.$option"
    done
}
mapfile -d '' dev_options < <(options "${DEV_OPTIONS:-}")
mapfile -d '' base_options < <(options "${BASE_OPTIONS:-}")

cd "$work"
fastchess \
    -engine cmd=./dev name=dev "${dev_options[@]}" \
    -engine cmd=./base name=base "${base_options[@]}" \
    -each tc="${TC:-8+0.08}" \
    -openings file="$book" format=pgn order=random \
    -repeat -games 2 -rounds 50000 \
    -concurrency "${CONCURRENCY:-$(nproc)}" \
    -draw movenumber=40 movecount=8 score=10 \
    -resign movecount=3 score=600 \
    -sprt elo0="${ELO0:-0}" elo1="${ELO1:-5}" alpha=0.05 beta=0.05 \
    -pgnout file=sprt.pgn append=false \
    "$@"
