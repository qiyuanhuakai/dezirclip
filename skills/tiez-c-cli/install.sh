#!/bin/bash
# Symlink tiez-c-cli skill to ~/.claude/skills/tiez-c-cli
set -e
SKILL_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="$HOME/.claude/skills/tiez-c-cli"
if [[ "$1" == "--uninstall" ]]; then
    rm -f "$TARGET"
    echo "Uninstalled: $TARGET"
    exit 0
fi
mkdir -p "$HOME/.claude/skills"
ln -sf "$SKILL_DIR" "$TARGET"
echo "Installed: $TARGET -> $SKILL_DIR"
echo "Reload any active Claude sessions to pick up the new skill."
