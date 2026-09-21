#!/usr/bin/env bash
# Fix: Delete garbled comments and re-send correct UTF-8 comments via curl
# Uses Git Bash + curl to avoid PowerShell encoding issues entirely.

set -euo pipefail

AUTH="$LINEAR_API_KEY"
PROJECT_ID="72e576a7-e894-4e22-b52c-5d47d43a5912"
API="https://api.linear.app/graphql"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COMMENTS_FILE="$SCRIPT_DIR/linear-comments.json"

linear_post() {
  local payload="$1"
  curl -sf -X POST "$API" \
    -H "Authorization: $AUTH" \
    -H "Content-Type: application/json; charset=utf-8" \
    -d "$payload"
}

get_comment() {
  local title="$1"
  if [[ "$title" =~ ^RD-M4-0(19|2[0-9]|3[0-9]) ]]; then
    jq -r '.workspace_domain' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M4-0(3[1-9]|4[0-6]) ]]; then
    jq -r '.file_watcher' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^QA-M4- ]]; then
    jq -r '.qa_m4' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^GATE-M4 ]]; then
    jq -r '.gate_m4' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M5-00[1-9] ]] || [[ "$title" =~ ^RD-M5-01[0-4] ]]; then
    jq -r '.ui_infra' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M5-01[5-9] ]] || [[ "$title" =~ ^RD-M5-02[0-9] ]] || [[ "$title" =~ ^RD-M5-030$ ]]; then
    jq -r '.task_tree' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M5-03[1-9] ]] || [[ "$title" =~ ^RD-M5-040$ ]]; then
    jq -r '.inspector' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M5-04[1-9] ]] || [[ "$title" =~ ^RD-M5-05[0-2] ]]; then
    jq -r '.keyboard' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^RD-M5-05[3-9] ]] || [[ "$title" =~ ^RD-M5-06[0-4] ]]; then
    jq -r '.filter' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^QA-M5- ]]; then
    jq -r '.qa_m5' "$COMMENTS_FILE"
  elif [[ "$title" =~ ^GATE-M5 ]]; then
    jq -r '.gate_m5' "$COMMENTS_FILE"
  else
    jq -r '.ui_infra' "$COMMENTS_FILE"
  fi
}

# Step 1: Fetch all Done issues
echo "=== Fetching Done issues ==="
ALL_ISSUES="[]"
HAS_NEXT=true
AFTER=""

while $HAS_NEXT; do
  if [ -n "$AFTER" ]; then
    CURSOR_PART=", after: \"$AFTER\""
  else
    CURSOR_PART=""
  fi

  QUERY="{\"query\": \"query { project(id: \\\"$PROJECT_ID\\\") { issues(first: 100$CURSOR_PART) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }\"}"
  RESP=$(linear_post "$QUERY")

  NODES=$(echo "$RESP" | jq -c '.data.project.issues.nodes')
  ALL_ISSUES=$(echo "$ALL_ISSUES $NODES" | jq -s 'add')

  HAS_NEXT=$(echo "$RESP" | jq '.data.project.issues.pageInfo.hasNextPage')
  AFTER=$(echo "$RESP" | jq -r '.data.project.issues.pageInfo.endCursor')
  [ "$HAS_NEXT" = "null" ] && HAS_NEXT=false
done

# Filter to M4/M5 Done issues
DONE_ISSUES=$(echo "$ALL_ISSUES" | jq -c '[.[] | select(
  (.title | test("^RD-M[45]-|^QA-M[45]-|^GATE-M[45]|^M[45] Epic")) and
  (.state.name == "Done")
)]')

TOTAL=$(echo "$DONE_ISSUES" | jq 'length')
echo "=== Found $TOTAL Done tasks ==="

# Step 2: Process each issue
UPDATED=0
FAILED=0

for i in $(seq 0 $((TOTAL - 1))); do
  ISSUE=$(echo "$DONE_ISSUES" | jq -c ".[$i]")
  ID=$(echo "$ISSUE" | jq -r '.id')
  IDENTIFIER=$(echo "$ISSUE" | jq -r '.identifier')
  TITLE=$(echo "$ISSUE" | jq -r '.title')

  echo ""
  echo "Processing $IDENTIFIER: $TITLE"

  # List existing comments
  LIST_PAYLOAD="{\"query\": \"query(\\\$id: String!) { issue(id: \\\$id) { comments { nodes { id body createdAt } } } }\", \"variables\": {\"id\": \"$ID\"}}"
  LIST_RESP=$(linear_post "$LIST_PAYLOAD")
  COMMENTS=$(echo "$LIST_RESP" | jq -c '.data.issue.comments.nodes // []')
  COMMENT_COUNT=$(echo "$COMMENTS" | jq 'length')

  # Delete garbled comments
  DELETED=0
  for j in $(seq 0 $((COMMENT_COUNT - 1))); do
    C=$(echo "$COMMENTS" | jq -c ".[$j]")
    CID=$(echo "$C" | jq -r '.id')
    CBODY=$(echo "$C" | jq -r '.body')

    IS_GARBLED=false

    # Pattern 1: ?? question marks (first sync failure) - header "## ?? ??????"
    if echo "$CBODY" | head -1 | grep -qP '^## \?\? \?\?\?\?\?\?'; then
      IS_GARBLED=true
    fi

    # Pattern 2: Mojibake - header has garbled CJK but NOT valid "## 📋 进度更新" or "##  进度更新"
    if [ "$IS_GARBLED" = "false" ]; then
      FIRST_LINE=$(echo "$CBODY" | head -1)
      # Check if it has our section structure (indicating it's one of our comments)
      if echo "$CBODY" | grep -q '实现方案\|遇到的问题\|解决方案'; then
        # It's our comment template, but check if the H2 header is garbled
        if ! echo "$FIRST_LINE" | grep -qP '^## [📋]?\s*进度更新'; then
          IS_GARBLED=true
        fi
      fi
    fi

    if [ "$IS_GARBLED" = "true" ]; then
      DEL_PAYLOAD="{\"query\": \"mutation(\\\$id: String!) { commentDelete(id: \\\$id) { success } }\", \"variables\": {\"id\": \"$CID\"}}"
      DEL_RESP=$(linear_post "$DEL_PAYLOAD")
      DEL_OK=$(echo "$DEL_RESP" | jq -r '.data.commentDelete.success // false')
      if [ "$DEL_OK" = "true" ]; then
        echo "  Deleted garbled comment ($CID)"
        DELETED=$((DELETED + 1))
      fi
      sleep 0.2
    fi
  done

  # Send correct comment
  COMMENT=$(get_comment "$TITLE")
  # Escape for JSON
  COMMENT_ESCAPED=$(echo "$COMMENT" | jq -Rs '.')
  ADD_PAYLOAD="{\"query\": \"mutation(\\\$issueId: String!, \\\$body: String!) { commentCreate(input: { issueId: \\\$issueId, body: \\\$body }) { success } }\", \"variables\": {\"issueId\": \"$ID\", \"body\": $COMMENT_ESCAPED}}"
  ADD_RESP=$(linear_post "$ADD_PAYLOAD")
  ADD_OK=$(echo "$ADD_RESP" | jq -r '.data.commentCreate.success // false')

  if [ "$ADD_OK" = "true" ]; then
    echo "  Sent correct comment ($DELETED garbled deleted)"
    UPDATED=$((UPDATED + 1))
  else
    echo "  FAIL: comment create failed"
    FAILED=$((FAILED + 1))
  fi
  sleep 0.2
done

echo ""
echo "=== Fix complete: $UPDATED success, $FAILED failed out of $TOTAL tasks ==="
