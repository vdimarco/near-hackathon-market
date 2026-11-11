#!/bin/sh
# Test of scripts/nofile_clamp.c and scripts/with_sandbox.sh.
#
# Run: sh scripts/test_nofile_clamp.sh
# Needs Linux with glibc, a C compiler, and python3.
#
# Each case lowers the limit of open files in a subshell. A process can always
# lower its own limit, so the test shows the refusal of a small container on
# any Linux machine.
set -eu
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Linux" ]; then
  echo "skipped: the limit library is for Linux"
  exit 0
fi

LOW=4096
passed=0

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

ok() {
  passed=$((passed + 1))
  printf '   %-66s ok\n' "$1"
}

# ask <call> <resource> <soft> <hard>
# Sends one request to the C library and prints the limits after it,
# or "refused" when the system says no.
ASK='
import ctypes, resource, sys
call, name, soft, hard = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
res = getattr(resource, "RLIMIT_" + name)
class Limit(ctypes.Structure):
    _fields_ = [("cur", ctypes.c_ulong), ("max", ctypes.c_ulong)]
libc = ctypes.CDLL(None, use_errno=True)
if getattr(libc, call)(res, ctypes.byref(Limit(soft, hard))) != 0:
    print("refused")
else:
    print(*resource.getrlimit(res))
'
ask() {
  python3 -c "$ASK" "$@"
}

echo "== The wrapper on a machine with a low limit"

note=$( (ulimit -n $LOW; sh scripts/with_sandbox.sh true) 2>&1 ) || fail "the wrapper stopped: $note"
case "$note" in
  *"$LOW"*"65535"*) ok "it prints a note with both numbers" ;;
  *) fail "expected a note with $LOW and 65535, got: $note" ;;
esac

LIB=$( (ulimit -n $LOW; sh scripts/with_sandbox.sh sh -c 'printf %s "$LD_PRELOAD"') 2>/dev/null ) || fail "the wrapper stopped"
case "$LIB" in
  /*/nofile_clamp.so) ok "it preloads the library by its full path" ;;
  *) fail "expected a full path to nofile_clamp.so in LD_PRELOAD, got: $LIB" ;;
esac
[ -f "$LIB" ] || fail "the library file does not exist: $LIB"

echo "== The library"

got=$( (ulimit -n $LOW; ask setrlimit NOFILE 65535 65535) )
[ "$got" = "refused" ] || fail "with no library the system must refuse 65535, got: $got"
ok "with no library the system refuses 65535 open files"

got=$( (ulimit -n $LOW; LD_PRELOAD=$LIB ask setrlimit NOFILE 65535 65535) )
[ "$got" = "$LOW $LOW" ] || fail "setrlimit: expected $LOW $LOW, got: $got"
ok "setrlimit: a request for 65535 becomes the hard limit"

got=$( (ulimit -n $LOW; LD_PRELOAD=$LIB ask setrlimit64 NOFILE 65535 65535) )
[ "$got" = "$LOW $LOW" ] || fail "setrlimit64: expected $LOW $LOW, got: $got"
ok "setrlimit64: a request for 65535 becomes the hard limit"

got=$( (ulimit -n $LOW; LD_PRELOAD=$LIB ask setrlimit NOFILE 1024 2048) )
[ "$got" = "1024 2048" ] || fail "expected 1024 2048, got: $got"
ok "a request under the hard limit stays as asked"

got=$( (ulimit -n $LOW; LD_PRELOAD=$LIB ask setrlimit CORE 0 0) )
[ "$got" = "0 0" ] || fail "expected 0 0 for the core file limit, got: $got"
ok "a request for another limit passes through"

echo "== The wrapper, other cases"

got=$( (ulimit -n $LOW; NEAR_SANDBOX_NOFILE=1024 sh scripts/with_sandbox.sh sh -c 'printf %s "${LD_PRELOAD:-none}"') 2>&1 )
[ "$got" = "none" ] || fail "with enough open files the wrapper must add nothing, got: $got"
ok "with enough open files it adds nothing and prints nothing"

set +e
(ulimit -n $LOW; sh scripts/with_sandbox.sh sh -c 'exit 7') 2>/dev/null
code=$?
set -e
[ "$code" = "7" ] || fail "expected exit code 7 from the command, got: $code"
ok "it returns the exit code of the command"

echo "$passed checks passed"
