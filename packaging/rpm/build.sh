#!/bin/sh
# Build dist/weirdnet-relay-VERSION-1.ARCH.rpm for each ARCH from dist/weirdnet-relay-VERSION-ARCH-linux.tar.gz.
# Needs rpmbuild (Fedora: dnf install rpm-build). Usage: packaging/rpm/build.sh VERSION ARCH...
set -eu
V=$1; shift
D=$(pwd)/dist
for ARCH in "$@"; do
  rpmbuild -bb --target "$ARCH" --define "ver $V" --define "_sourcedir $D" \
    --define "_rpmdir $D/rpm" --define "_topdir $(mktemp -d)" packaging/rpm/weirdnet-relay.spec
done
find "$D/rpm" -name '*.rpm' -exec mv {} "$D/" \;
rm -rf "$D/rpm"
ls "$D"/*.rpm
