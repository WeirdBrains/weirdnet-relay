# Changelog

All notable changes to weirdnet-relay are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.6.0] - 2026-11-19

### Changed

- Breaking (wire): built on weirdnet-net 0.8.0. The names server is `/weirdnet/names/2` and the relay's own requests are `/weirdnet/relay/info/2`, so only 0.8 nodes register and look up names here.
- Circuits, Kademlia, identify, wake and update streams are unchanged.

## [0.5.0] - 2026-11-16

### Added

- Dynamic DNS through Grindvakt: the relay keeps its name and `_dnsaddr` records pointed at its public address, on DigitalOcean or DNSimple. Off by default.
- The `ddns` command shows the DNS changes, and `--apply` writes them.
- check-config reads the DNS token when dynamic DNS is on.

## [0.4.0] - 2026-11-16

### Added

- Update announcements: the relay carries signed announcements for the orgs it trusts, keeps the latest, and follows other relays.
- The info document says whether the relay carries update announcements.

## [0.3.0] - 2026-11-16

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
- Example and packaged configs join Weirdnet through the [WeirdBrains](https://weirdbrains.com) public relay.

[Unreleased]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/WeirdBrains/weirdnet-relay/releases/tag/v0.1.0
