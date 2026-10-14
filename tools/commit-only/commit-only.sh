#!/usr/bin/env bash
# commit-only -- <message> <path>...: commits the named paths alone, new, changed or deleted, and leaves every
# other change of the tree, staged or not, as it was, so a session commits its own work while another session
# works in the same tree. A commit of git add -A would take the work of the other one too.
set -euo pipefail
if [ "$#" -lt 2 ]; then
  echo "commit-only: give the message, then every path to commit" >&2
  exit 2
fi
message=$1
shift
git add -A -- "$@"
git commit --quiet --only -m "$message" -- "$@"
git log --oneline -n 1
