#!/bin/sh
# Runs a command that starts the NEAR sandbox.
# Usage: sh scripts/with_sandbox.sh <command> [arguments]
#
# The sandbox binary asks the system for 65,535 open files when it starts, and
# it stops if the system refuses:
#
#   Error: couldn't set the file descriptor limit to (65535, 65535), ...
#
# Most machines accept the request. Some containers have a lower hard limit
# and do not let a process raise it. On such a machine this script builds
# scripts/nofile_clamp.c into the build folder and preloads it. The library
# lowers the request to the hard limit of the machine. On every other machine
# this script runs the command unchanged and prints nothing.
#
# NEAR_SANDBOX_NOFILE sets the number that the sandbox asks for. The default
# is 65535, which is the number in near-sandbox 2.14.
set -eu

need=${NEAR_SANDBOX_NOFILE:-65535}
hard=$(ulimit -Hn)

if [ "$hard" = "unlimited" ] || [ "$hard" -ge "$need" ]; then
  exec "$@"
fi

if [ "$(uname -s)" != "Linux" ]; then
  echo "The NEAR sandbox asks for $need open files, and this machine allows $hard." >&2
  echo "Raise the limit, then run the command again: ulimit -n $need" >&2
  exit 1
fi

here=$(cd "$(dirname "$0")" && pwd)
out=${CARGO_TARGET_DIR:-target}
mkdir -p "$out"
out=$(cd "$out" && pwd)
lib="$out/nofile_clamp.so"

if [ ! -f "$lib" ] || [ "$here/nofile_clamp.c" -nt "$lib" ]; then
  if ! ${CC:-cc} -shared -fPIC -O2 -o "$lib" "$here/nofile_clamp.c" -ldl; then
    echo "The NEAR sandbox asks for $need open files, and this machine allows $hard." >&2
    echo "The build of scripts/nofile_clamp.c failed, so the sandbox cannot start here." >&2
    exit 1
  fi
fi

echo "note: this machine allows $hard open files, and the NEAR sandbox asks for $need." >&2
echo "note: preloading $lib, which lowers the request to $hard." >&2

LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}"
export LD_PRELOAD
exec "$@"
