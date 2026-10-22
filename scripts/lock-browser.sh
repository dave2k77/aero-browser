#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'Usage: %s TRUSTED_BROWSER_EXECUTABLE\n' "$0" >&2
  printf 'Checks the reviewed development pin; does not download or approve a browser.\n' >&2
  exit 2
fi

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
status=0
report="$(cargo run --quiet --locked --manifest-path "$root/Cargo.toml" -p aero-cli -- \
  doctor --json --chromium "$1")" || status=$?
if [[ $status -ne 0 && $status -ne 2 ]]; then
  printf 'Cannot run browser pin check (exit %s).\n' "$status" >&2
  exit "$status"
fi

printf '%s\n' "$report" | python -c '
import json, sys
report = json.load(sys.stdin)
if report["browser"]["available"] and report["browser_lock"]["status"] == "matched_local_pin":
    print("PASS: browser version/executable match the development pin; sandbox remains unverified.")
else:
    print("FAIL: browser unavailable or does not match the reviewed development pin.", file=sys.stderr)
    sys.exit(1)
'
