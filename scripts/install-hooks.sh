#!/usr/bin/env bash
set -euo pipefail

# Configure this repository to use the bundled hooks in .githooks/
# Run this once after cloning: `scripts/install-hooks.sh`

git config core.hooksPath .githooks
printf "Installed git hooks (core.hooksPath -> .githooks)\n"
printf "To undo: git config --unset core.hooksPath\n"
