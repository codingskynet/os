#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 4 ]; then
    echo "usage: $0 TIMEOUT LOG MARKER COMMAND [ARG ...]" >&2
    exit 2
fi

timeout_seconds=$1
log=$2
marker=$3
shift 3

: >"$log"
# Preserve stdin explicitly: Bash otherwise redirects an asynchronous command's
# stdin from /dev/null when job control is disabled.
exec 3<&0
timeout "${timeout_seconds}s" "$@" <&3 >"$log" 2>&1 &
runner_pid=$!
exec 3<&-
matched=0

while kill -0 "$runner_pid" 2>/dev/null; do
    if grep -Fq -- "$marker" "$log"; then
        matched=1
        kill -TERM "$runner_pid" 2>/dev/null || true
        break
    fi
    sleep 0.2
done

set +e
wait "$runner_pid" 2>/dev/null
status=$?
set -e

# Catch a marker written immediately before the command exited.
if [ "$matched" -eq 0 ] && grep -Fq -- "$marker" "$log"; then
    matched=1
fi

cat "$log"

if [ "$matched" -eq 1 ]; then
    exit 0
fi

echo "Expected output was not observed within ${timeout_seconds}s: $marker" >&2
if [ "$status" -ne 0 ]; then
    exit "$status"
fi
exit 1
