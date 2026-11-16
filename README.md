# weirdnet-relay

A relay and bootstrap node for the [Weirdnet](https://net.weirdbrains.com) network, built on
[weirdnet-net](https://github.com/WeirdBrains/weirdnet-net).

Apps on Weirdnet (Morph, Muninn as `weirdnet-muninn`, …) share the base protocols
`/weirdnet/kad/1`, `/weirdnet/id/1` and `/weirdnet/relay/1` on port 4747. A device behind a
NAT reserves a slot on a relay; others reach it at `<relay>/p2p-circuit/p2p/<device>` and
then try to switch to a direct path (DCUtR). The relay is also a Kademlia server, so a new
node can bootstrap from it, and it provides the `/weirdnet/relay/1` key so nodes find relays.

What it does:

- **Circuit relay v2** with **no caps by default**: no reservation or circuit counts, no
  circuit duration or byte limit, no rate limiters. `limits.preset = "libp2p"` starts from
  libp2p's defaults instead (2 minutes and 128 KiB per circuit); any field can be set.
- **Kademlia bootstrap** (server mode), identify, ping, AutoNAT, DCUtR, mDNS (off by default).
- **One encrypted database**: the node key and a table of peers seen, in SQLCipher, keyed
  from a systemd credential.
- **An info page** describing the relay: HTML at `/`, JSON at
  `/.well-known/weirdnet-relay.json`, health at `/healthz`, and the same document over the
  `/weirdnet/relay/info/1` protocol.

## Quick start

    cargo install --git https://github.com/WeirdBrains/weirdnet-relay
    umask 077; weirdnet-relay gen-key > db-key
    weirdnet-relay --db-key-file db-key --state-dir ./state \
        --external /dns4/relay.example.com/tcp/4747 --contact abuse@example.com

Then open http://127.0.0.1:4780/. Open TCP and UDP 4747 in the firewall.

## Commands

| Command | |
|---|---|
| `run` (default) | Run the relay and the info page until SIGINT or SIGTERM. |
| `id` | Print the peer id (creates the node key on first use). |
| `info` | Start briefly and print the info document. |
| `check-config` | Check the config, open the database with the key, exit. |
| `gen-key` | Print a new database key (64 hex characters) to stdout. |

Every config key has a flag and an environment variable: `--config` / `WEIRDNET_RELAY_CONFIG`,
`--name`, `--operator`, `--contact`, `--state-dir`, `--db-key-file`, `--port`, `--listen`,
`--external`, `--bootstrap` (comma-separated lists in the environment), `--mdns`,
`--http-listen`, `--no-http`, `--limits unlimited|libp2p`, `--log-format pretty|json`,
`--log-level`, each as `WEIRDNET_RELAY_<NAME>`. Flags win over the environment, which wins
over the file. See [examples/relay.toml](examples/relay.toml) for the file.

## The database key

`relay.db` (in `state_dir`, default `$STATE_DIRECTORY` then `/var/lib/weirdnet-relay`) is a
SQLCipher database. Its key is 32 bytes, written as 64 hex characters (or 32 raw bytes) in a
file. The relay looks, in order, at `db_key_file` / `--db-key-file`, then the systemd
credential `$CREDENTIALS_DIRECTORY/db-key`. With systemd:

    weirdnet-relay gen-key | systemd-creds encrypt --name=db-key - /etc/weirdnet-relay/db-key.cred

and the unit's `LoadCredentialEncrypted=db-key:/etc/weirdnet-relay/db-key.cred`
hands it to the service. Lose the key and the relay gets a new peer id; keep a copy offline.

## systemd

[packaging/systemd/weirdnet-relay.service](packaging/systemd/weirdnet-relay.service) runs the
relay as a dynamic user with a private state directory, no capabilities, a read-only system,
and a `@system-service` syscall filter. Logs go to the journal (no timestamps there); set
`log.format = "json"` for structured logs.

## The info document

    curl -s https://relay.example.com/.well-known/weirdnet-relay.json

```json
{
  "v": 1,
  "name": "Example Weirdnet relay",
  "contact": "abuse@example.com",
  "software": "weirdnet-relay/0.1.0",
  "peer_id": "12D3KooW…",
  "addrs": ["/dns4/relay.example.com/tcp/4747/p2p/12D3KooW…"],
  "transports": ["tcp", "quic-v1"],
  "roles": ["relay", "bootstrap", "kad-server"],
  "network": { "profile": "weirdnet-relay", "kad": "/weirdnet/kad/1", "identify": "/weirdnet/id/1",
               "relay_key": "/weirdnet/relay/1", "info_protocol": "/weirdnet/relay/info/1", "default_port": 4747 },
  "limits": { "max_circuits": null, "max_circuit_duration_secs": null, "max_circuit_bytes": null, "rate_limits": false, "…": "…" },
  "status": { "nat": "public", "connected_peers": 12, "started_at": 0, "generated_at": 0 }
}
```

`null` limits mean no limit. Every address ends in `/p2p/<peer_id>`; `RelayInfo::parse`
refuses a document that doesn't. Put a TLS reverse proxy in front of `127.0.0.1:4780` to
publish the page. `v` only changes on a breaking change.

## Building

    cargo build --release

Edition 2024, Rust 1.99+. `cargo test` includes a loopback test that runs a real relay over
TCP, reserves a slot, carries a 1 MiB answer through the circuit and asks for the info.

## License

BSD-3-Clause.
