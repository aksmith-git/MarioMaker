#!/bin/sh
# Run a command for at most SECS seconds (macOS has no timeout(1)): run_for.sh SECS cmd args...
# Exit status: the command's, or 124 if it was stopped.
secs=$1; shift
"$@" &
pid=$!
( sleep "$secs"; kill -TERM "$pid" 2>/dev/null; sleep 3; kill -KILL "$pid" 2>/dev/null ) &
watcher=$!
wait "$pid"
st=$?
if kill -0 "$watcher" 2>/dev/null; then kill "$watcher" 2>/dev/null; else st=124; fi
exit $st
