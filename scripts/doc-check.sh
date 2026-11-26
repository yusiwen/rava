#!/usr/bin/env bash
# doc-check.sh — hold the ledger to its shape.
#
# Usage: doc-check.sh [TARGET_DIR]
#
# Exit codes: 0 = the documents are within budget, 1 = a budget is broken.
#
# The budgets exist because PROJECT.md grew to 1357 lines by absorbing every slice's
# implementation narrative (2026-11-26). A line budget is what makes that regression fail
# on CI instead of in review: evidence belongs in docs/slices/, one line per slice in the
# ledger.

set -uo pipefail

TARGET="${1:-.}"
TARGET="${TARGET%/}"
[ -n "$TARGET" ] || TARGET="/"

LIMIT_LINES=200
LIMIT_LINE_CHARS=200
LIMIT_RESUME_LINES=25

PROJECT="$TARGET/PROJECT.md"
SLICES_DIR="$TARGET/docs/slices"

FAILS=0
pass() { printf 'PASS  %s\n' "$1"; }
fail() { FAILS=$((FAILS + 1)); printf 'FAIL  %s\n' "$1"; }

printf 'doc-check: target %s\n\n' "$TARGET"

if [ ! -f "$PROJECT" ]; then
  fail "PROJECT.md not found at $PROJECT"
  printf '\n%d failure(s)\n' "$FAILS"
  exit 1
fi
pass "PROJECT.md found"

# ---- line budget -------------------------------------------------------------
LINES=$(wc -l < "$PROJECT" | tr -d ' ')
if [ "$LINES" -le "$LIMIT_LINES" ]; then
  pass "PROJECT.md is $LINES lines (limit $LIMIT_LINES)"
else
  fail "PROJECT.md is $LINES lines (limit $LIMIT_LINES): move detail into docs/slices/"
fi

# ---- per-line budget ---------------------------------------------------------
LONGEST=$(awk '{ if (length($0) > m) { m = length($0); n = NR } } END { print m + 0 "\t" n + 0 }' "$PROJECT")
LONGEST_LEN=${LONGEST%%	*}
LONGEST_LINE=${LONGEST##*	}
if [ "$LONGEST_LEN" -le "$LIMIT_LINE_CHARS" ]; then
  pass "longest line is $LONGEST_LEN characters (limit $LIMIT_LINE_CHARS)"
else
  fail "line $LONGEST_LINE is $LONGEST_LEN characters (limit $LIMIT_LINE_CHARS): evidence goes in the slice record, not a table cell"
fi

# ---- Resume budget -----------------------------------------------------------
RESUME_LINES=$(awk '/^##[[:space:]]+Resume/{f=1;next} /^##[[:space:]]/{f=0} f && NF' "$PROJECT" | wc -l | tr -d ' ')
if [ "$RESUME_LINES" -le "$LIMIT_RESUME_LINES" ]; then
  pass "Resume is $RESUME_LINES non-empty lines (limit $LIMIT_RESUME_LINES)"
else
  fail "Resume is $RESUME_LINES non-empty lines (limit $LIMIT_RESUME_LINES): it is a status card, history goes in docs/slices/ and docs/notes/"
fi

# ---- every done slice has a record -------------------------------------------
if [ ! -d "$SLICES_DIR" ]; then
  fail "docs/slices/ is missing: per-slice records have nowhere to live"
else
  DONE_IDS=$(awk '/^##[[:space:]]+Slices/{f=1;next} /^##[[:space:]]/{f=0} f' "$PROJECT" |
    awk -F'|' '/^\|/ {
      id = $2; status = $6
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", id)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", status)
      if (id ~ /^[MS][0-9]/ && status == "done") print id
    }')
  # Headings inside fenced code blocks are examples, not records.
  HEADINGS=$(awk '/^```/ { fence = !fence; next } !fence && /^## / { print }' "$SLICES_DIR"/*.md 2>/dev/null)
  MISSING=""
  COUNT=0
  for id in $DONE_IDS; do
    COUNT=$((COUNT + 1))
    if ! printf '%s\n' "$HEADINGS" | grep -qF -- "## $id — "; then
      MISSING="$MISSING $id"
    fi
  done
  if [ -z "$MISSING" ]; then
    pass "every done slice has a record ($COUNT checked)"
  else
    fail "done slice(s) with no '## <id> — ' entry in docs/slices/:$MISSING"
  fi
fi

# ---- record shape (issue #64) ------------------------------------------------
# A record is "## M<id> — …" up to the next "## ". Twelve non-empty lines and 200
# characters per line is what keeps a record scannable; the evidence it used to carry
# lives in the explanation document it links to.
SHAPE_BAD=""
for f in "$SLICES_DIR"/M*.md; do
  [ -f "$f" ] || continue
  bad=$(awk '
    function flush() { if (id != "" && n > 12) print id " (" n ")" }
    /^## / { flush(); id = ""; n = 0; if ($2 ~ /^M[0-9]/) id = $2; next }
    id != "" && NF { n++ }
    END { flush() }' "$f")
  [ -n "$bad" ] && SHAPE_BAD="$SHAPE_BAD $bad"
done
LONG_LINES=$(awk 'length($0) > 200 { print FILENAME ":" FNR }' "$SLICES_DIR"/M*.md 2>/dev/null | tr '\n' ' ')
if [ -z "$SHAPE_BAD" ] && [ -z "$LONG_LINES" ]; then
  pass "every slice record is within 12 lines and 200 characters"
else
  [ -n "$SHAPE_BAD" ] && fail "slice record(s) over 12 non-empty lines:$SHAPE_BAD"
  [ -n "$LONG_LINES" ] && fail "slice record line(s) over 200 characters: $LONG_LINES"
fi

# ---- relative links resolve --------------------------------------------------
BROKEN=""
while IFS= read -r link; do
  [ -n "$link" ] || continue
  if [ ! -e "$TARGET/$link" ]; then
    BROKEN="$BROKEN $link"
  fi
done <<LINKS
$(grep -o '](\([^)]*\))' "$PROJECT" | sed 's/^](//; s/)$//' | grep -v '^\(https\?://\|mailto:\|#\)' | sed 's/#.*$//' | sort -u)
LINKS
if [ -z "$BROKEN" ]; then
  pass "every relative link in PROJECT.md resolves"
else
  fail "broken link(s) in PROJECT.md:$BROKEN"
fi

printf '\n%d failure(s)\n' "$FAILS"
[ "$FAILS" -eq 0 ] && exit 0
exit 1
