#!/usr/bin/env bash
# Usage: run-codex.sh <model> <effort> <name> [cwd]
# Reads prompt from $S/<name>.prompt.md, writes final message to $S/<name>.out.md and full log to $S/<name>.log
# Optional CODEX_SANDBOX (e.g. danger-full-access): the default workspace-write sandbox on this
# Windows machine blocks git (CreateFileMapping), node spawn and loopback tests, so coders that
# must run the full check list need it.
set -u
S="${CODEX_SCRATCH:?set CODEX_SCRATCH to a folder holding <name>.prompt.md}"
MODEL="$1"; EFFORT="$2"; NAME="$3"; DIR="${4:-$(git rev-parse --show-toplevel)}"
cd "$DIR" || exit 1
codex exec --skip-git-repo-check -m "$MODEL" -c model_reasoning_effort="$EFFORT" \
  ${CODEX_SANDBOX:+-s "$CODEX_SANDBOX"} \
  -o "$S/$NAME.out.md" - < "$S/$NAME.prompt.md" > "$S/$NAME.log" 2>&1
code=$?
echo "codex exit $code"
tail -c 600 "$S/$NAME.out.md" 2>/dev/null
