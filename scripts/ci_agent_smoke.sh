#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-38173}"
LOG_FILE="${TMPDIR:-/tmp}/procsentinel-agent-${PORT}.log"
PROC_FILE="${TMPDIR:-/tmp}/procsentinel-processes-${PORT}.json"

./target/debug/procsentinel --agent --port "$PORT" >"$LOG_FILE" 2>&1 &
AGENT_PID=$!
cleanup() {
  kill "$AGENT_PID" 2>/dev/null || true
  wait "$AGENT_PID" 2>/dev/null || true
  rm -f "$PROC_FILE"
}
trap cleanup EXIT

ready=0
for _ in $(seq 1 40); do
  if curl --fail --silent "http://127.0.0.1:${PORT}/api/health" >/dev/null; then
    ready=1
    break
  fi
  if ! kill -0 "$AGENT_PID" 2>/dev/null; then
    cat "$LOG_FILE" >&2 || true
    echo "agent exited before becoming ready" >&2
    exit 1
  fi
  sleep 0.25
done

if [[ "$ready" -ne 1 ]]; then
  cat "$LOG_FILE" >&2 || true
  echo "agent did not become ready" >&2
  exit 1
fi

curl --fail --silent "http://127.0.0.1:${PORT}/api/processes" >"$PROC_FILE"
python3 - "$PROC_FILE" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as f:
    payload = json.load(f)
if not isinstance(payload, list):
    raise SystemExit('process endpoint did not return a JSON array')
PY

if command -v ss >/dev/null 2>&1; then
  listeners="$(ss -ltn)"
  grep -Fq "127.0.0.1:${PORT}" <<<"$listeners" || {
    echo "expected loopback listener was not found" >&2
    echo "$listeners" >&2
    exit 1
  }
  if grep -Eq "(^|[[:space:]])(0\.0\.0\.0|\[::\]):${PORT}([[:space:]]|$)" <<<"$listeners"; then
    echo "agent unexpectedly exposed a wildcard listener" >&2
    exit 1
  fi
fi

echo "loopback agent health/process smoke test passed"
