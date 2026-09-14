#!/bin/bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "APFS case-mode test is macOS-only" >&2
  exit 0
fi

root="$(mktemp -d "${RUNNER_TEMP:-/tmp}/slink-apfs.XXXXXX")"
insensitive_mount="$root/insensitive"
sensitive_mount="$root/sensitive"
insensitive_image="$root/insensitive.sparseimage"
sensitive_image="$root/sensitive.sparseimage"
mkdir "$insensitive_mount" "$sensitive_mount"

cleanup() {
  hdiutil detach "$sensitive_mount" >/dev/null 2>&1 || true
  hdiutil detach "$insensitive_mount" >/dev/null 2>&1 || true
  rm -rf "$root"
}
trap cleanup EXIT

hdiutil create -quiet -type SPARSE -size 32m -fs APFS \
  -volname slink-ci-insensitive "$insensitive_image"
hdiutil attach -quiet -nobrowse -mountpoint "$insensitive_mount" "$insensitive_image"

hdiutil create -quiet -type SPARSE -size 32m -fs 'Case-sensitive APFS' \
  -volname slink-ci-sensitive "$sensitive_image"
hdiutil attach -quiet -nobrowse -mountpoint "$sensitive_mount" "$sensitive_image"

write_registry() {
  local mount="$1"
  mkdir -p "$mount/config/slink"
  cat >"$mount/config/slink/links.toml" <<EOF
[[link]]
link = "$mount/Alpha"
target = "$mount/one"

[[link]]
link = "$mount/alpha"
target = "$mount/two"
EOF
}

write_registry "$insensitive_mount"
write_registry "$sensitive_mount"

if XDG_CONFIG_HOME="$insensitive_mount/config" target/debug/slink fix -n >/dev/null 2>&1; then
  echo "case-insensitive APFS accepted duplicate destinations" >&2
  exit 1
fi

XDG_CONFIG_HOME="$sensitive_mount/config" target/debug/slink fix -n >/dev/null
