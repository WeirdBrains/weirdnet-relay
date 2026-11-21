# Handoff

Running notes for [weirdnet-relay](https://github.com/WeirdBrains/weirdnet-relay), newest first.

## 2026-11-21: v0.6.1, public-content patch release

- What changed: v0.6.1 republishes the 0.6 line with the public-content rules applied (Weird Brains LLC as the copyright holder, m3talsmith as the author, no machine details), on top of `53d3e36`. No code or API changes. Its exact pins move to the republished weirdnet-net 0.8.2, weirdnet-storage 0.1.1, weirdnet-push 0.1.1 and weirdnet-update 0.2.2.
- Commits: `53d3e36` and the `release: v0.6.1` commit tagged [v0.6.1](https://github.com/WeirdBrains/weirdnet-relay/releases/tag/v0.6.1).
- Checks: cargo fmt, clippy, test and package; the packaged crate was scanned for private terms before publishing.
- Not tested: nothing beyond the crate's own test suite.
- Left: earlier versions on this line are yanked on [crates.io](https://crates.io/crates/weirdnet-relay/versions).
- Next: nothing for this release.

