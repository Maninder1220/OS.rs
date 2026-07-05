#!/usr/bin/env bash
# =============================================================================
# File: scripts/build-rpm.sh
# Purpose:
#   Builds an RPM package for installing OSAI Agent on RPM-based Linux systems.
#
# Where this fits in OSAI:
#   Uses packaging/rpm/osai-agent.spec after compiling release binaries.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   Packaging should happen after Rust builds pass and systemd files are verified.
# =============================================================================
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAME="osai-agent"
VERSION="0.2.0"
RPMTOP="${RPMTOP:-$HOME/rpmbuild}"

mkdir -p "$RPMTOP"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

rsync -a \
  --exclude target \
  --exclude data \
  --exclude .git \
  "$PROJECT_ROOT/" "$TMPDIR/$NAME-$VERSION/"

tar -C "$TMPDIR" -czf "$RPMTOP/SOURCES/$NAME-$VERSION.tar.gz" "$NAME-$VERSION"
cp "$PROJECT_ROOT/packaging/rpm/osai-agent.spec" "$RPMTOP/SPECS/osai-agent.spec"

rpmbuild -ba "$RPMTOP/SPECS/osai-agent.spec"
