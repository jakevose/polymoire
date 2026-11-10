#!/usr/bin/env bash
# Dev runner: runs allowlisted `mise run <task>` requests written to .runner/request, so a
# tool that can only write files in this folder (like Claude) can build and test on this
# machine while you watch. Nothing outside the allowlist runs. Stop it with Ctrl-C.
set -u
cd "$(dirname "$0")/.." || exit 1
dir=.runner
mkdir -p "$dir"
allowed=" build test lint fmt ci smoke check "

if ! command -v mise >/dev/null 2>&1; then
  echo "mise is not installed. Install it from https://mise.jdx.dev then rerun this script."
  exit 1
fi
mise trust --quiet . >/dev/null 2>&1 || mise trust .
echo "Installing the pinned Rust toolchain (first run only)…"
mise install

echo "Driftfield dev runner is waiting for requests (Ctrl-C to stop)."
while true; do
  if [ -f "$dir/request" ]; then
    task=$(tr -d '[:space:]' < "$dir/request")
    rm -f "$dir/request" "$dir/exit"
    if [[ "$allowed" != *" $task "* ]]; then
      echo "Refused unknown task: $task" | tee "$dir/log"
      echo 127 > "$dir/exit"
      continue
    fi
    echo "▶ $(date +%H:%M:%S) mise run $task"
    mise run "$task" > "$dir/log" 2>&1
    code=$?
    echo "$code" > "$dir/exit"
    echo "  finished with exit code $code"
  fi
  sleep 1
done
