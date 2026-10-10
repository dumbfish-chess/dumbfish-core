#!/usr/bin/env bash
set -euo pipefail
unset SYZYGY_PATH

usage="usage: scripts/calibrate.sh [<ref>]

Measures the Elo of every Skill Level of dumbfish built from <ref> (the working tree if omitted)
and writes the table for UCI_Elo. Levels go from the strongest down. Each plays Stockfish with
UCI_LimitStrength at an Elo near its own, found by a short probe match against Stockfish at the
Elo of the level above (\$ANCHOR for level 20); a level weaker than Stockfish can play (1320)
plays the level above instead. Neighbouring levels can differ by less than the error, so levels
out of order are averaged together (isotonic regression) in the table.

Matches run in chunks of \$CHUNK games, each with its own log under
target/calibrate/<bench signature>-<hash of skill.rs>/. A rerun skips finished chunks, so an
interrupted calibration resumes, and a larger \$GAMES only adds chunks. A chunk with a time loss
(the computer slept, or other work took the CPU) is played again, up to twice. Needs stockfish
and fastchess in PATH; nothing else should load the CPU meanwhile.

Environment (defaults):
  GAMES=200 (per level)  CHUNK=50  PROBE=20  ANCHOR=2300
  TC=10+0.1  CONCURRENCY=\$(nproc)  OUT=engine/skill-elo.txt"

[[ ${1:-} != -h && ${1:-} != --help ]] || { echo "$usage" >&2; exit 1; }
ref=${1:-}
games=${GAMES:-200}
chunk=${CHUNK:-50}
probe=${PROBE:-20}
anchor=${ANCHOR:-2300}
sf_min=1320
sf_max=3190

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
    skill=$(git -C "$root" rev-parse "$ref:engine/src/skill.rs")
else
    cargo build --release -p dumbfish --manifest-path "$root/Cargo.toml" --target-dir "$work/target"
    skill=$(git -C "$root" hash-object "$root/engine/src/skill.rs")
fi
bin=$work/dumbfish
cp -f "$work/target/release/dumbfish" "$bin"
runs=$work/$("$bin" bench | awk '{print $1}')-${skill:0:8}
mkdir -p "$runs"
echo "results in $runs"

engine() {
    case $1 in
        sf) printf '%s\0' -engine cmd=stockfish name=stockfish option.Threads=1 option.Hash=16 \
            option.UCI_LimitStrength=true "option.UCI_Elo=$2" ;;
        level) printf '%s\0' -engine "cmd=$bin" "name=level$2" "option.Skill Level=$2" ;;
    esac
}

play() {
    local dir=$1 name=$2 games=$3 level=$4 seed=$5
    shift 5
    [[ -f $dir/$name.result ]] && return
    local attempt timeouts result
    mapfile -d '' us < <(engine level "$level")
    mapfile -d '' them < <(engine "$@")
    for attempt in 1 2 3; do
        (
            cd "$dir"
            fastchess "${us[@]}" "${them[@]}" \
                -each tc="${TC:-10+0.1}" \
                -openings file="$book" format=pgn order=random -srand "$seed" \
                -repeat -games 2 -rounds $((games / 2)) \
                -concurrency "${CONCURRENCY:-$(nproc)}" \
                -pgnout file="$name.pgn" append=false
        ) > "$dir/$name.log" 2>&1 || true
        if ! grep -q '^Finished match' "$dir/$name.log"; then
            echo "$dir/$name: fastchess did not finish, see its log" >&2
            exit 1
        fi
        timeouts=$(awk '/^ *Timeouts:/ { n += $2 } END { print n + 0 }' "$dir/$name.log")
        result=$(sed -nE 's/^Games: [0-9]+, Wins: ([0-9]+), Losses: ([0-9]+), Draws: ([0-9]+).*/\1 \3 \2/p' \
            "$dir/$name.log" | tail -1)
        if ((timeouts == 0 || attempt == 3)); then
            ((timeouts == 0)) || echo "warning: $dir/$name kept with $timeouts time losses" >&2
            echo "$result" > "$dir/$name.result"
            return
        fi
        echo "$dir/$name: $timeouts time losses, playing it again" >&2
    done
}

elo_diff() {
    awk '{ w += $1; d += $2; l += $3 } END {
        n = w + d + l; s = (w + d / 2) / n
        if (s < 0.5 / n) s = 0.5 / n
        if (s > 1 - 0.5 / n) s = 1 - 0.5 / n
        var = (w * (1 - s) ^ 2 + d * (0.5 - s) ^ 2 + l * s ^ 2) / n
        err = 1.96 * sqrt(var / n) * 400 / log(10) / (s * (1 - s))
        printf "%.0f %.0f %d\n", -400 * log(1 / s - 1) / log(10), err, n
    }' "$@"
}

clamp() {
    awk -v e="$1" -v lo="$sf_min" -v hi="$sf_max" \
        'BEGIN { e = int(e / 10 + 0.5) * 10; print (e < lo ? lo : e > hi ? hi : e) }'
}

declare -A elo
for ((level = 20; level >= 0; level--)); do
    dir=$runs/level$level
    mkdir -p "$dir"
    if [[ ! -f $dir/opponent ]]; then
        if ((level < 20)) && ((elo[$((level + 1))] < sf_min + 200)); then
            echo "level $((level + 1))" > "$dir/opponent"
        else
            guess=$(clamp "${elo[$((level + 1))]:-$anchor}")
            play "$dir" probe "$probe" "$level" $((1000 * level)) sf "$guess"
            read -r diff _ _ < <(elo_diff "$dir/probe.result")
            target=$((guess + diff))
            if ((target >= sf_min)); then
                echo "sf $(clamp "$target")" > "$dir/opponent"
            else
                echo "level $((level + 1))" > "$dir/opponent"
            fi
        fi
    fi
    read -r kind opponent < "$dir/opponent"
    for ((c = 1; c <= games / chunk; c++)); do
        play "$dir" "chunk$c" "$chunk" "$level" $((1000 * level + c)) "$kind" "$opponent"
    done
    read -r diff err n < <(elo_diff "$dir"/chunk*.result)
    base=$([[ $kind == sf ]] && echo "$opponent" || echo "${elo[$opponent]}")
    elo[$level]=$((base + diff))
    echo "level $level: ${elo[$level]} +/- $err ($diff in $n games against $kind $opponent)"
done

{
    echo "# level  elo  (scripts/calibrate.sh: Stockfish UCI_Elo opponents, tc ${TC:-10+0.1}, $games games per level)"
    for ((level = 0; level <= 20; level++)); do
        echo "$level ${elo[$level]}"
    done | awk '
        { level[NR] = $1; sum[++blocks] = $2; size[blocks] = 1
          while (blocks > 1 && sum[blocks - 1] / size[blocks - 1] > sum[blocks] / size[blocks]) {
              sum[blocks - 1] += sum[blocks]; size[blocks - 1] += size[blocks]; blocks--
          } }
        END { for (b = 1; b <= blocks; b++)
                  for (i = 0; i < size[b]; i++)
                      printf "%-6s %5d\n", level[++n], sum[b] / size[b] + 0.5 }'
} > "$out"
echo "wrote $out; levels out of order within the error are averaged:"
grep -v '^#' "$out" | tr '\n' ' '
echo
