#!/bin/sh
# Build dist/weirdnet-relay-PKGVER-1-ARCH.pkg.tar.zst for each ARCH with makepkg, from the recipe
# recipe.py writes with the hashes of the tarballs in dist/, and the TAUR recipe archive
# dist/weirdnet-relay-PKGVER-taur.tar.gz. Runs on Arch; as root (CI containers) it does the work
# as a throwaway build user, since makepkg refuses root. Nothing is compiled, so both arches
# build on x86_64.  Usage: packaging/arch/build.sh VERSION ARCH...
set -eu
V=$1; shift
if [ "$(id -u)" = 0 ]; then
  id builder >/dev/null 2>&1 || useradd -m builder
  S=$(mktemp -d)
  mkdir -p "$S/dist"
  cp -r packaging scripts "$S/"
  cp dist/weirdnet-relay-"$V"-*-linux.tar.gz "$S/dist/"
  chown -R builder "$S"
  su builder -c "cd '$S' && packaging/arch/build.sh '$V' $*"
  cp "$S"/dist/*.pkg.tar.zst "$S"/dist/*-taur.tar.gz dist/
  rm -rf "$S"
  exit 0
fi
D=$(pwd)/dist
W=$(mktemp -d)
(cd "$D" && sha256sum weirdnet-relay-"$V"-*-linux.tar.gz) > "$W/sums"
python3 packaging/arch/recipe.py "$V" "$W/recipe" --sums "$W/sums" --archive "$D"
cp "$D"/weirdnet-relay-"$V"-*-linux.tar.gz "$W/recipe/"
for ARCH in "$@"; do
  sed -e "s/^CARCH=.*/CARCH=\"$ARCH\"/" -e "s/^CHOST=.*/CHOST=\"$ARCH-pc-linux-gnu\"/" /etc/makepkg.conf > "$W/makepkg-$ARCH.conf"
  (cd "$W/recipe" && PKGDEST="$D" makepkg --config "$W/makepkg-$ARCH.conf" -f --noconfirm --nodeps)
done
rm -rf "$W"
ls "$D"/*.pkg.tar.zst "$D"/*-taur.tar.gz
