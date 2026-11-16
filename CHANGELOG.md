# Changelog

All notable changes to weirdnet-relay are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The wake path for phones without APNs or FCM, with an optional push service to follow.
- The info document says whether the relay carries wakes.

### Changed

- Builds on weirdnet-net 0.4.0 and weirdnet-push 0.1.0.

## [0.2.0] - 2026-11-16

### Added

- WebSocket over TLS for apps on networks that only let HTTPS out, served by the relay or behind a TLS proxy.
- Public HTTPS forwarding for Weirdnet hosts by SNI passthrough, off by default.
- revoke-site, unrevoke-site and revoked-sites commands.
- The info document says whether the relay forwards sites.

## [0.1.1] - 2026-11-16

### Added

- RPM and Arch Linux packages for x86_64 and aarch64, and the Arch recipe on TAUR.
- Install steps for apt, dnf, zypper, pacman and TAUR.

### Changed

- Release tarballs hold the packaged config, with the full example under examples/.

## [0.1.0] - 2026-11-16

### Added

- Relay and bootstrap node for Weirdnet, with no relay caps by default.
- Encrypted relay database, keyed from a systemd credential.
- Info page in HTML and JSON, a health check and an info protocol.
- Names server for signed peer records.
- Command line: run, id, info, check-config and gen-key.
- Hardened systemd unit and a Debian package.
- Deployment guide for relay.net.weirdbrains.com.
- Example and packaged configs join Weirdnet through the WeirdBrains public relay.

[Unreleased]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/WeirdBrains/weirdnet-relay/releases/tag/v0.1.0
