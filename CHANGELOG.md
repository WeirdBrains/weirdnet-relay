# Changelog

## 0.1.0 (unreleased)

- First release: a circuit relay v2 and Kademlia bootstrap node on weirdnet-net, on the
  Weirdnet base protocols (`/weirdnet/kad/1`, `/weirdnet/id/1`, `/weirdnet/relay/1`, port 4747).
- No caps by default (`limits.preset = "unlimited"`: no reservation or circuit counts, no
  circuit duration or byte cap, no rate limiters); `preset = "libp2p"` and per-field overrides.
- The node key and the peer table in one SQLCipher database (weirdnet-storage); the database
  key comes from the systemd credential `db-key` or a key file.
- An info page: HTML at `/`, JSON at `/.well-known/weirdnet-relay.json`, `/healthz`, and the
  same document over `/weirdnet/relay/info/1`.
- CLI: `run`, `id`, `info`, `check-config`, `gen-key`; TOML config, flags and `WEIRDNET_RELAY_*`.
- A hardened systemd unit, and DEPLOY.md for relay.net.weirdbrains.com.
- A names (rendezvous) server for signed peer records, stored in the relay's database.
