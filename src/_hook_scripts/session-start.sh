#!/bin/bash
# @handbook 8.2-hook-script-invariants
# @handbook 8.3-pid-from-ppid
# @tested tests/hook_scripts.rs
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[ -z "$SESSION_ID" ] && exit 0

REGISTRY="$HOME/.claude/duru/registry/${SESSION_ID}.json"
mkdir -p "$(dirname "$REGISTRY")"
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
SOURCE=$(echo "$INPUT" | jq -r '.source // empty')
MODE=$(echo "$INPUT" | jq -r '.permission_mode // empty')
# $PPID here maps directly to the long-running `claude` process — Claude
# Code invokes hooks via exec, not via an intermediary shell. Verified in #16.
PID_VAL="${PPID:-0}"

TMP=$(mktemp "${REGISTRY}.XXXXXX")
jq -n \
  --arg sid "$SESSION_ID" --arg hb "$NOW" --arg cwd "$CWD" \
  --arg tr "$TRANSCRIPT" --arg src "$SOURCE" --arg mode "$MODE" \
  --argjson pid "$PID_VAL" \
  '{schema_version:1, session_id:$sid, pid:$pid, cwd:$cwd,
    transcript_path:$tr, started_at:$hb,
    source:(if $src == "" then null else $src end),
    last_heartbeat:$hb,
    permission_mode:(if $mode == "" then null else $mode end),
    terminated:false}' \
  > "$TMP"
mv "$TMP" "$REGISTRY"
exit 0
