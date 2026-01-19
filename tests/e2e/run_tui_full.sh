#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TEMPLATE="$SCRIPT_DIR/tui_full.yaml"

if ! command -v termwright >/dev/null 2>&1; then
  echo "termwright not found. Install with: cargo install termwright" >&2
  exit 1
fi

CONDUIT_BINARY="${CONDUIT_BINARY:-}"
if [ -z "$CONDUIT_BINARY" ]; then
  if [ -x "$PROJECT_ROOT/target/debug/conduit" ]; then
    CONDUIT_BINARY="$PROJECT_ROOT/target/debug/conduit"
  elif [ -x "$PROJECT_ROOT/target/release/conduit" ]; then
    CONDUIT_BINARY="$PROJECT_ROOT/target/release/conduit"
  else
    echo "Conduit binary not found. Build with: cargo build" >&2
    exit 1
  fi
fi

# Expectations for the step file:
# - HOME points at a temp dir.
# - ~/code exists and contains repo-alpha and repo-beta.
# - Conduit uses the default base dir (~/code) without manual typing.
DATA_DIR="$(mktemp -d -t conduit-e2e-data-XXXXXX)"
TMP_HOME="$(mktemp -d -t conduit-e2e-home-XXXXXX)"
STEP_FILE="$(mktemp -t conduit-e2e-steps-XXXXXX.yaml)"

cleanup() {
  rm -f "$STEP_FILE"
  rm -rf "$DATA_DIR" "$TMP_HOME"
}
trap cleanup EXIT

create_repo() {
  local path="$1"
  local name
  name="$(basename "$path")"
  mkdir -p "$path"
  git -C "$path" init -q
  printf "# %s\n" "$name" > "$path/README.md"
  git -C "$path" add README.md
  git -C "$path" -c user.name="Conduit E2E" -c user.email="conduit-e2e@example.com" \
    commit -q -m "init"
}

export HOME="$TMP_HOME"

BASE_DIR="$HOME/code"
mkdir -p "$BASE_DIR"

create_repo "$BASE_DIR/repo-alpha"
create_repo "$BASE_DIR/repo-beta"

sed \
  -e "s|__CONDUIT_BINARY__|$CONDUIT_BINARY|g" \
  -e "s|__DATA_DIR__|$DATA_DIR|g" \
  "$TEMPLATE" > "$STEP_FILE"

termwright run-steps --trace "$STEP_FILE"
