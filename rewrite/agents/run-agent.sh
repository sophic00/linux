#!/usr/bin/env bash
# Spawn a role agent (pi session) for the Rust rewrite program.
#
# Usage: run-agent.sh <name> <cwd> <task-prompt-file> [extra pi args...]
#
# The agent runs non-interactively with the same model as the orchestrator
# session (inherited via PI_MODEL/PI_PROVIDER), project-local tooling and
# skills loaded, in the given working directory (usually a git worktree).
# Output goes to /tmp/rw-agents/<name>.log; PID to /tmp/rw-agents/<name>.pid.
set -euo pipefail

name="$1"; cwd="$2"; taskfile="$3"; shift 3 || true
logdir=/tmp/rw-agents
mkdir -p "$logdir"

provider="${PI_PROVIDER:-opencode}"
model="${PI_MODEL:?PI_MODEL must be set (run from orchestrator session)}"

nohup pi -p \
    --provider "$provider" --model "$model" \
    -a -n "rw:$name" \
    --session-dir "$logdir/sessions" \
    "$(cat "$taskfile")" "$@" \
    >"$logdir/$name.log" 2>&1 &

echo $! > "$logdir/$name.pid"
echo "spawned '$name' pid $(cat "$logdir/$name.pid") log=$logdir/$name.log"
