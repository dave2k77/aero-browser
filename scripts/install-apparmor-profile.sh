#!/usr/bin/env bash
# Installs or removes AeroBrowser's sandbox launcher and its AppArmor profile.
# This changes host security policy and must be run by the machine's owner:
#
#   sudo bash scripts/install-apparmor-profile.sh install [GROUP]
#   sudo bash scripts/install-apparmor-profile.sh remove
#
# install: copies /usr/bin/bwrap to a root-owned launcher that only GROUP may
# execute, and loads packaging/apparmor/aerobrowser-bwrap for that path. Run it
# again after a bubblewrap package update to refresh the copy.
# remove: unloads the profile and deletes everything install created.
# See the profile's header and tracker ENV-005/DEC-014 for what this permits.
set -euo pipefail

launcher_dir=/usr/local/libexec/aerobrowser
launcher="$launcher_dir/bwrap"
installed_profile=/etc/apparmor.d/aerobrowser-bwrap
system_bwrap=/usr/bin/bwrap
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
profile="$root/packaging/apparmor/aerobrowser-bwrap"

usage() {
  printf 'Usage: sudo bash %s install [GROUP] | remove\n' "$0" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
action="$1"
if [[ "$(id -u)" -ne 0 ]]; then
  printf 'This changes host security policy and must run as root (use sudo).\n' >&2
  exit 2
fi
if ! command -v apparmor_parser >/dev/null 2>&1; then
  printf 'apparmor_parser is required (package: apparmor).\n' >&2
  exit 2
fi

case "$action" in
  install)
    group="${2:-}"
    if [[ -z "$group" ]]; then
      if [[ -z "${SUDO_GID:-}" ]]; then
        printf 'Name the group allowed to use the launcher: install GROUP\n' >&2
        exit 2
      fi
      group="$(getent group "$SUDO_GID" | cut -d: -f1)"
    fi
    if ! getent group "$group" >/dev/null; then
      printf 'Unknown group: %s\n' "$group" >&2
      exit 2
    fi
    if [[ "$group" == root ]]; then
      printf 'Refusing group root: name the developer group.\n' >&2
      exit 2
    fi
    if [[ ! -f "$profile" || ! -x "$system_bwrap" ]]; then
      printf 'Missing %s or %s.\n' "$profile" "$system_bwrap" >&2
      exit 1
    fi
    # Check the profile compiles before touching the system.
    apparmor_parser --skip-kernel-load --skip-cache "$profile"
    install -d -o root -g "$group" -m 0750 "$launcher_dir"
    install -o root -g root -m 0755 "$system_bwrap" "$launcher"
    install -o root -g root -m 0644 "$profile" "$installed_profile"
    apparmor_parser --replace "$installed_profile"
    printf 'Installed %s (group %s) and loaded profile aerobrowser-bwrap.\n' "$launcher" "$group"
    printf 'Use it with: export AERO_BWRAP=%s\n' "$launcher"
    ;;
  remove)
    [[ $# -eq 1 ]] || usage
    if [[ -f "$installed_profile" ]]; then
      apparmor_parser --remove "$installed_profile" || true
      rm -f -- "$installed_profile"
    fi
    rm -f -- "$launcher"
    if [[ -d "$launcher_dir" ]]; then
      rmdir -- "$launcher_dir"
    fi
    printf 'Removed the launcher and the aerobrowser-bwrap profile.\n'
    ;;
  *)
    usage
    ;;
esac
