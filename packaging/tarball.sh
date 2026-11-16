#!/bin/sh
# dist/weirdnet-relay-VERSION-ARCH-linux.tar.gz from a release binary. The same files the .deb
# installs, laid out for the RPM spec and the Arch PKGBUILD:
#   weirdnet-relay  systemd/weirdnet-relay.service  relay.toml (-> /etc/weirdnet-relay/)
#   examples/relay.toml  README.md DEPLOY.md CHANGELOG.md LICENSE
# Usage: packaging/tarball.sh VERSION ARCH [BINARY]
set -eu
V=$1 ARCH=$2 BIN=${3:-target/release/weirdnet-relay}
T=weirdnet-relay-$V-$ARCH-linux
mkdir -p dist
W=$(mktemp -d)
mkdir -p "$W/$T/systemd" "$W/$T/examples"
install -m755 "$BIN" "$W/$T/weirdnet-relay"
install -m644 README.md DEPLOY.md CHANGELOG.md LICENSE "$W/$T/"
install -m644 packaging/relay.toml "$W/$T/relay.toml"
install -m644 examples/relay.toml "$W/$T/examples/relay.toml"
install -m644 packaging/systemd/weirdnet-relay.service "$W/$T/systemd/"
tar -C "$W" --owner=0 --group=0 --numeric-owner --sort=name -czf "dist/$T.tar.gz" "$T"
rm -rf "$W"
echo "dist/$T.tar.gz"
