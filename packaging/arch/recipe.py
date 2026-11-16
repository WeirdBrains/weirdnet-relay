#!/usr/bin/env python3
"""Write the weirdnet-relay Arch recipe for a release: PKGBUILD (pkgver set, sha256sums filled),
weirdnet-relay.install and .SRCINFO in OUT_DIR, and with --archive the TAUR upload archive
(weirdnet-relay-PKGVER-taur.tar.gz) in that directory.

    recipe.py VERSION OUT_DIR [--sums FILE] [--archive DIR]

The hashes come from FILE (sha256sum lines: HASH  NAME) or, without --sums, from the release's
SHA256SUMS on GitHub. Uses the vendored shared release script for pkgver, .SRCINFO and packing.
"""
import sys

sys.dont_write_bytecode = True

import argparse
import importlib.util
import re
import shutil
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
REPO = "https://github.com/WeirdBrains/weirdnet-relay"
ARCHES = ("x86_64", "aarch64")


def shared():
    spec = importlib.util.spec_from_file_location("weirdnet_release", ROOT / "scripts" / "weirdnet_release.py")
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def sums(text):
    out = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2 and re.fullmatch(r"[0-9a-f]{64}", parts[0]):
            out[parts[1].lstrip("*")] = parts[0]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("version")
    ap.add_argument("out")
    ap.add_argument("--sums")
    ap.add_argument("--archive")
    a = ap.parse_args()
    rel = shared()
    v = a.version.lstrip("v")
    if v == "0.1.0" and not a.sums:
        sys.exit("0.1.0's release tarballs predate the package layout (no examples/, no packaged relay.toml); "
                 "the first release for TAUR is 0.1.1")
    if a.sums:
        have = sums(Path(a.sums).read_text())
    else:
        with urllib.request.urlopen(f"{REPO}/releases/download/v{v}/SHA256SUMS", timeout=60) as r:
            have = sums(r.read().decode())
    text = (HERE / "PKGBUILD").read_text()
    text = re.sub(r"^pkgver=.*$", f"pkgver={rel.pkgver(v)}", text, count=1, flags=re.M)
    text = re.sub(r"^pkgrel=.*$", "pkgrel=1", text, count=1, flags=re.M)
    # The tarballs are named by the release version (0.2.0-alpha.1), not pkgver (0.2.0alpha1).
    text = text.replace("v${pkgver}/weirdnet-relay-${pkgver}-", f"v{v}/weirdnet-relay-{v}-")
    text = text.replace('cd "${srcdir}/weirdnet-relay-${pkgver}-', f'cd "${{srcdir}}/weirdnet-relay-{v}-')
    for arch in ARCHES:
        name = f"weirdnet-relay-{v}-{arch}-linux.tar.gz"
        if name not in have:
            sys.exit(f"no sha256 for {name}")
        text, n = re.subn(rf"^sha256sums_{arch}=\(.*\)$", f"sha256sums_{arch}=('{have[name]}')", text, flags=re.M)
        if n != 1:
            sys.exit(f"PKGBUILD has no sha256sums_{arch} line")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "PKGBUILD").write_text(text)
    shutil.copyfile(HERE / "weirdnet-relay.install", out / "weirdnet-relay.install")
    (out / ".SRCINFO").write_text(rel.make_srcinfo(out, "auto"))
    errs = rel.taur_check(v, out, {"name": "weirdnet-relay"})
    if errs:
        sys.exit("recipe not ready:\n  " + "\n  ".join(errs))
    print(out)
    if a.archive:
        archive, _ = rel.taur_pack(out, a.archive, v)
        print(archive)


if __name__ == "__main__":
    main()
