#!/bin/bash
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[ -z "$SESSION_ID" ] && exit 0

REGISTRY="$HOME/.claude/duru/registry/${SESSION_ID}.json"
[ ! -f "$REGISTRY" ] && exit 0
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
REASON=$(echo "$INPUT" | jq -r '.reason // "other"')

TMP=$(mktemp "${REGISTRY}.XXXXXX")
jq --arg t "$NOW" --arg r "$REASON" \
  '.terminated = true | .ended_at = $t | .end_reason = $r | .last_heartbeat = $t' \
  "$REGISTRY" > "$TMP"
mv "$TMP" "$REGISTRY"
exit 0
