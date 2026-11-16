#!/bin/sh
# Install test for a built package, run as root on its distro (CI runs it in fedora and
# archlinux containers; the .deb test runs on the runner itself, with systemd):
#   packaging/test-install.sh rpm dist/weirdnet-relay-X.Y.Z-1.x86_64.rpm
#   packaging/test-install.sh arch dist/weirdnet-relay-X.Y.Z-1-x86_64.pkg.tar.zst
# Checks the files, that the relay is not enabled without /etc/weirdnet-relay/db-key.cred,
# that check-config passes, that a changed relay.toml survives a reinstall, and removal.
set -eu
KIND=$1 PKG=$2
case "$KIND" in
  rpm)  list() { rpm -qlp "$1"; }; install_() { dnf install -y -q "$1"; }; again() { rpm -Uvh --replacepkgs --nosignature "$1"; }; remove() { dnf remove -y -q weirdnet-relay; } ;;
  arch) list() { pacman -Qlpq "$1"; }; install_() { pacman -U --noconfirm "$1"; }; again() { pacman -U --noconfirm "$1"; }; remove() { pacman -R --noconfirm weirdnet-relay; } ;;
  *) echo "kind is rpm or arch" >&2; exit 2 ;;
esac
fail() { echo "FAIL: $*" >&2; exit 1; }
# Docs are checked in the package listing: container images skip /usr/share/doc on install.
L=$(list "$PKG")
for f in /usr/bin/weirdnet-relay /usr/lib/systemd/system/weirdnet-relay.service /etc/weirdnet-relay/relay.toml \
  /usr/share/doc/weirdnet-relay/README.md /usr/share/doc/weirdnet-relay/DEPLOY.md \
  /usr/share/doc/weirdnet-relay/CHANGELOG.md /usr/share/doc/weirdnet-relay/examples/relay.toml; do
  echo "$L" | grep -qx "$f" || fail "$f is not in the package"
done
echo "$L" | grep -q 'licenses/weirdnet-relay/LICENSE$' || fail "LICENSE is not in the package"
install_ "$PKG"
weirdnet-relay --version
for f in /usr/bin/weirdnet-relay /usr/lib/systemd/system/weirdnet-relay.service /etc/weirdnet-relay/relay.toml; do
  test -f "$f" || fail "missing $f"
done
grep -q '^DynamicUser=yes' /usr/lib/systemd/system/weirdnet-relay.service || fail "unit is not the hardened one"
grep -q 'relay.net.weirdbrains.com' /etc/weirdnet-relay/relay.toml || fail "relay.toml is not the packaged config"
# No db-key credential: the package must not have enabled the relay.
test ! -e /etc/systemd/system/multi-user.target.wants/weirdnet-relay.service || fail "enabled without db-key.cred"
T=$(mktemp -d); (umask 077; weirdnet-relay gen-key > "$T/key")
weirdnet-relay --config /etc/weirdnet-relay/relay.toml --state-dir "$T/state" --db-key-file "$T/key" check-config
rm -rf "$T"
# A changed config is kept on reinstall/upgrade.
echo '# local change' >> /etc/weirdnet-relay/relay.toml
again "$PKG"
grep -q '^# local change' /etc/weirdnet-relay/relay.toml || fail "relay.toml was replaced on reinstall"
remove
test ! -e /usr/bin/weirdnet-relay || fail "binary left after removal"
echo "install test ok: $PKG"
