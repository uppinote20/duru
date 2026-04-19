#!/bin/bash
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[ -z "$SESSION_ID" ] && exit 0

REGISTRY="$HOME/.claude/duru/registry/${SESSION_ID}.json"
# Ensure the registry dir exists before mktemp — SessionStart may have been
# skipped (for instance on `claude --resume` the first time hooks are seen).
mkdir -p "$(dirname "$REGISTRY")"
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
MODE=$(echo "$INPUT" | jq -r '.permission_mode // empty')

TMP=$(mktemp "${REGISTRY}.XXXXXX")
if [ -f "$REGISTRY" ]; then
  jq --arg hb "$NOW" --arg mode "$MODE" \
    '.last_heartbeat = $hb | if $mode != "" then .permission_mode = $mode else . end' \
    "$REGISTRY" > "$TMP"
else
  CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
  TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
  jq -n --arg sid "$SESSION_ID" --arg hb "$NOW" --arg mode "$MODE" \
        --arg cwd "$CWD" --arg tr "$TRANSCRIPT" --argjson pid "${PPID:-0}" \
    '{schema_version:1, session_id:$sid, pid:$pid, cwd:$cwd,
      transcript_path:$tr, started_at:$hb, last_heartbeat:$hb,
      permission_mode:(if $mode == "" then null else $mode end),
      terminated:false}' \
    > "$TMP"
fi
mv "$TMP" "$REGISTRY"
exit 0
