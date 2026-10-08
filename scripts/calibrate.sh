#!/usr/bin/env bash
set -euo pipefail

usage="usage: scripts/calibrate.sh [<ref>]

Measures the Elo of every Skill Level of dumbfish built from <ref> (the working tree if omitted)
and writes the table for UCI_Elo. Level 20 is anchored by a match against Stockfish with
UCI_LimitStrength at UCI_Elo=\$ANCHOR; lower levels follow a ladder of self-play matches
(18 vs 20, 16 vs 18, ..., 0 vs 2), odd levels are interpolated. Needs stockfish and fastchess
in PATH.

Environment (defaults):
  ANCHOR=1500  GAMES=200 (per match)  TC=10+0.1  CONCURRENCY=\$(nproc)
  OUT=engine/skill-elo.txt"

[[ ${1:-} != -h && ${1:-} != --help ]] || { echo "$usage" >&2; exit 1; }
ref=${1:-}
anchor=${ANCHOR:-1500}
games=${GAMES:-200}

root=$(git rev-parse --show-toplevel)
work=$root/target/calibrate
out=${OUT:-$root/engine/skill-elo.txt}
mkdir -p "$work"
book=$root/target/sprt/8moves_v3.pgn
if [[ ! -f $book ]]; then
    curl -fsSL -o "$book.zip" https://github.com/official-stockfish/books/raw/master/8moves_v3.pgn.zip
    unzip -o -q "$book.zip" -d "$root/target/sprt"
fi

if [[ -n $ref ]]; then
    src=$work/src
    git -C "$root" worktree remove --force "$src" 2>/dev/null || true
    git -C "$root" worktree add --detach --quiet "$src" "$ref"
    trap 'git -C "$root" worktree remove --force "$src"' EXIT
    cargo build --release -p dumbfish --manifest-path "$src/Cargo.toml" --target-dir "$work/target"
    cp -f "$work/target/release/dumbfish" "$work/dumbfish"
else
    cargo build --release -p dumbfish --manifest-path "$root/Cargo.toml"
    cp -f "$root/target/release/dumbfish" "$work/dumbfish"
fi

level() {
    printf '%s\0' -engine "cmd=$work/dumbfish" "name=level$1" "option.Skill Level=$1"
}

match() {
    local name=$1
    shift
    (
        cd "$work"
        fastchess "$@" \
            -each tc="${TC:-10+0.1}" \
            -openings file="$book" format=pgn order=random \
            -repeat -games 2 -rounds $((games / 2)) \
            -concurrency "${CONCURRENCY:-$(nproc)}" \
            -pgnout file="$name.pgn" append=false
    ) > "$work/$name.log" 2>&1
    local diff
    diff=$(grep -E '^Elo: ' "$work/$name.log" | tail -1 | awk '{print $2}')
    if [[ ! $diff =~ ^-?[0-9]+(\.[0-9]+)?$ ]]; then
        echo "$name: no finite Elo (${diff:-none}), see $work/$name.log" >&2
        exit 1
    fi
    echo "$diff"
}

declare -A elo
mapfile -d '' strongest < <(level 20)
diff=$(match anchor "${strongest[@]}" \
    -engine cmd=stockfish name=stockfish option.Threads=1 option.Hash=16 \
    option.UCI_LimitStrength=true option.UCI_Elo="$anchor")
elo[20]=$(awk -v a="$anchor" -v d="$diff" 'BEGIN { printf "%d", a + d }')
echo "level 20: ${elo[20]} (${diff} against Stockfish UCI_Elo $anchor)"
if awk -v d="$diff" 'BEGIN { exit !(d > 300 || d < -300) }'; then
    echo "warning: far from the anchor, rerun with ANCHOR=${elo[20]} for a better estimate" >&2
fi

for ((l = 18; l >= 0; l -= 2)); do
    mapfile -d '' weaker < <(level "$l")
    mapfile -d '' stronger < <(level $((l + 2)))
    diff=$(match "level$l" "${weaker[@]}" "${stronger[@]}")
    elo[$l]=$(awk -v s="${elo[$((l + 2))]}" -v d="$diff" 'BEGIN { printf "%d", s + d }')
    echo "level $l: ${elo[$l]} (${diff} against level $((l + 2)))"
done
for ((l = 1; l < 20; l += 2)); do
    elo[$l]=$(( (elo[$((l - 1))] + elo[$((l + 1))]) / 2 ))
done
for ((l = 1; l <= 20; l++)); do
    if (( elo[l] <= elo[l - 1] )); then
        echo "warning: level $l is not stronger than level $((l - 1)), rerun with more GAMES" >&2
    fi
done

{
    echo "# level  elo  (scripts/calibrate.sh, anchor Stockfish UCI_Elo $anchor, tc ${TC:-10+0.1}, $games games per match)"
    for ((l = 0; l <= 20; l++)); do
        printf '%-6s %5s\n' "$l" "${elo[$l]}"
    done
} > "$out"
echo "wrote $out"
