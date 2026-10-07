bin := justfile_directory() / "target/play/dumbfish"

# Build dumbfish from <ref> and speak UCI on stdin/stdout; see `build` for <ref>
uci ref="newest": (build ref)
    @{{ bin }}

# Build dumbfish from <ref> into target/play/dumbfish, a stable path for a GUI; <ref> is a branch or commit, "newest" the most recently committed branch, "." the working tree
build ref="newest":
    #!/usr/bin/env bash
    set -euo pipefail
    root="{{ justfile_directory() }}"
    ref="{{ ref }}"
    if [[ $ref == newest ]]; then
        ref=$(git -C "$root" for-each-ref --sort=-committerdate --count=1 --format='%(refname:short)' refs/heads/)
    fi
    mkdir -p "$root/target/play"
    if [[ $ref == . ]]; then
        cargo build --release -q -p dumbfish --manifest-path "$root/Cargo.toml"
        cp -f "$root/target/release/dumbfish" "{{ bin }}"
        built="the working tree"
    else
        src="$root/target/play/src"
        git -C "$root" worktree remove --force "$src" 2>/dev/null || true
        git -C "$root" worktree add --detach --quiet "$src" "$ref"
        trap 'git -C "$root" worktree remove --force "$src"' EXIT
        cargo build --release -q -p dumbfish --manifest-path "$src/Cargo.toml" --target-dir "$root/target/play/target"
        cp -f "$root/target/play/target/release/dumbfish" "{{ bin }}"
        built="$ref ($(git -C "$root" log -1 --format='%h %s' "$ref"))"
    fi
    echo "built {{ bin }} from $built" >&2
