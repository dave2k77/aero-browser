# Source this file from the repository root to configure browser tests:
#
#   source scripts/dev-env.sh
#
# It exports AERO_CHROMIUM for the pinned development browser and, where
# AeroBrowser's AppArmor-profiled launcher is installed
# (scripts/install-apparmor-profile.sh), AERO_BWRAP. It installs and downloads
# nothing, and it reports what it set and what is missing.

_aero_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
_aero_version="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' \
  "$_aero_root/packaging/browser-lock.json")"
_aero_browser="$HOME/.cache/aerobrowser/browsers/$_aero_version/chrome-linux64/chrome"
_aero_launcher=/usr/local/libexec/aerobrowser/bwrap

if [ -x "$_aero_browser" ]; then
  export AERO_CHROMIUM="$_aero_browser"
  printf 'AERO_CHROMIUM=%s\n' "$AERO_CHROMIUM"
else
  printf 'Pinned browser %s is not provisioned at %s (see docs/development.md).\n' \
    "$_aero_version" "$_aero_browser" >&2
fi

if [ -x "$_aero_launcher" ]; then
  export AERO_BWRAP="$_aero_launcher"
  printf 'AERO_BWRAP=%s\n' "$AERO_BWRAP"
elif [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null)" = 1 ]; then
  printf 'AppArmor restricts user namespaces here and the AeroBrowser launcher is not installed;\n' >&2
  printf 'browser tests will fail with "No usable sandbox!". See docs/development.md.\n' >&2
else
  printf 'AERO_BWRAP not set; tests use /usr/bin/bwrap.\n'
fi

unset _aero_root _aero_version _aero_browser _aero_launcher
