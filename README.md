# weirdnet-relay

A relay and bootstrap node for the [Weirdnet](https://net.weirdbrains.com) network, built on
[weirdnet-net](https://github.com/WeirdBrains/weirdnet-net).

Apps on the Weirdnet profile (Morph, …) share the base protocols `/weirdnet/kad/1`,
`/weirdnet/id/1` and `/weirdnet/relay/1` on port 4747. A device behind a NAT reserves a slot
on a relay; others reach it at `<relay>/p2p-circuit/p2p/<device>` and then try to switch to a
direct path (DCUtR). The relay is also a Kademlia server, so a new node can bootstrap from it,
and it provides the `/weirdnet/relay/1` key so nodes find relays.

Apps with their own protocol ids can use it too. Muninn is on Weirdnet as `weirdnet-muninn`
and keeps its `/muninn/...` protocols: circuit relay v2, DCUtR and AutoNAT are libp2p's
standard protocols, so a Muninn node configured with this relay's full address
(`…/p2p/<relay peer id>`) can reserve a slot and be reached through it. The DHT and relay
discovery are per protocol id, so they serve `/weirdnet/...` nodes only.

What it does:

- **Circuit relay v2** with **no caps by default**: no reservation or circuit counts, no
  circuit duration or byte limit, no rate limiters. `limits.preset = "libp2p"` starts from
  libp2p's defaults instead (2 minutes and 128 KiB per circuit); any field can be set.
- **Kademlia bootstrap** (server mode), identify, ping, AutoNAT, DCUtR, mDNS (off by default).
- **One encrypted database**: the node key and a table of peers seen, in SQLCipher, keyed
  from a systemd credential.
- **A names server** (`/weirdnet/names/1`): devices register signed peer records with their
  home relay and others look them up by user id; records and withdrawals are checked by
  weirdnet-net and kept in the relay's database until they expire.
- **An info page** describing the relay: HTML at `/`, JSON at
  `/.well-known/weirdnet-relay.json`, health at `/healthz`, and the same document over the
  `/weirdnet/relay/info/1` protocol.

## The WeirdBrains public relay

WeirdBrains runs public relays. The first is **relay.net.weirdbrains.com**, *live*
([info page](https://relay.net.weirdbrains.com/)). Use it as a bootstrap node and relay:

    /dns4/relay.net.weirdbrains.com/tcp/4747/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB
    /dns4/relay.net.weirdbrains.com/udp/4747/quic-v1/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB

Its info document (`https://relay.net.weirdbrains.com/.well-known/weirdnet-relay.json`)
lists the same addresses. Abuse reports: michael+relay@weirdbrains.com.
[DEPLOY.md](DEPLOY.md) is how it is installed.

## Install

Each [release](https://github.com/WeirdBrains/weirdnet-relay/releases) has packages for x86_64
and aarch64: a `.deb` (Debian 12+, Ubuntu 24.04+: `apt-get install`), an `.rpm` (Fedora, RHEL
and openSUSE: `dnf install` or `zypper install`) and an Arch `.pkg.tar.zst` (`pacman -U`), plus
`tar.gz` builds. On Arch the recipe is also on
[TAUR](https://pkg.taur.weirdbrains.com/packages/weirdnet-relay), for paru or yay. Every package
installs the binary, the hardened systemd unit and `/etc/weirdnet-relay/relay.toml` (kept on
upgrade), and starts nothing until the database key credential exists.
[DEPLOY.md](DEPLOY.md) has the steps for each distro. The `.rpm` and Arch packages start with
0.1.1.

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
| `revoke-site NAME [--reason …]` | Stop forwarding a site (a running relay picks it up). |
| `unrevoke-site NAME` | Forward a revoked site again. |
| `revoked-sites` | List revoked sites. |

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

## WebSocket over TLS and site forwarding

`[wss]` lets apps on HTTPS-only networks dial the relay at `/dns4/<name>/tcp/443/tls/ws`, with
the relay holding the certificate or behind a TLS proxy. `[forwarding]` turns the relay into a
public HTTPS ingress for Weirdnet hosts without a public address, by SNI passthrough: TLS ends
on the host, each name needs its owner's signed grant and a `_weirdnet.<name>` DNS binding,
and `revoke-site` stops a site. Both are off by default; see DEPLOY.md (step 7) and
[examples/relay.toml](examples/relay.toml).

## Waking phones

`[wake]` (on by default) carries wakes for phones that can't use APNs or FCM. A phone that
opted in keeps a stream open to its home relay on `/weirdnet/wake/1` and subscribes its wake
handles; trusted peers deliver sealed, fixed-size payloads, and with `push` and
`push_token_file` set the relay also follows a push service's relay announcements. The relay
passes payloads on unread, holds them briefly for a phone that just dropped off, and caps
wakes per handle. The protocol is in
[weirdnet-push](https://github.com/WeirdBrains/weirdnet-push) (`relaywake`).

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
    cargo deb --locked -o dist/      # the .deb (needs cargo-deb and dpkg-dev)
    packaging/tarball.sh VERSION x86_64
    packaging/rpm/build.sh VERSION x86_64      # on Fedora, with rpm-build
    packaging/arch/build.sh VERSION x86_64     # on Arch, with base-devel; also the TAUR recipe
    packaging/test-install.sh rpm|arch FILE    # the install test CI runs in containers

Releases go through the shared WeirdBrains release script, `scripts/weirdnet_release.py`
(configured in `release.toml`); the Release workflow builds the packages for the tag and
attaches them to the GitHub release. Edition 2024, Rust 1.99+. `cargo test` includes a loopback test that runs a real relay over
TCP, reserves a slot, carries a 1 MiB answer through the circuit and asks for the info.

## License

BSD-3-Clause.
