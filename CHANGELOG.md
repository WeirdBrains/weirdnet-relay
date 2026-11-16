# Changelog

All notable changes to weirdnet-relay are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/WeirdBrains/weirdnet-relay/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/WeirdBrains/weirdnet-relay/releases/tag/v0.1.0
