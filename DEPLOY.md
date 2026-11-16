# Deploying the WeirdBrains public relay

The WeirdBrains public relay is **relay.net.weirdbrains.com**.
These steps install weirdnet-relay there with systemd. They work the same on any Linux
server with systemd 250 or newer (for `systemd-creds`).

Status: **coming online.** The DNS name is live; the relay is not deployed yet. Its peer id
is filled in below once the relay has made its key (`<RELAY_PEER_ID>` until then).

## 1. Install the .deb

Each release on https://github.com/WeirdBrains/weirdnet-relay/releases carries
`weirdnet-relay_<version>-1_amd64.deb` and `_arm64.deb` (Debian 12+, Ubuntu 24.04+: systemd 250 or newer), a
`SHA256SUMS` file and, when signed, `SHA256SUMS.minisig`:

    V=0.1.0; ARCH=$(dpkg --print-architecture)
    BASE=https://github.com/WeirdBrains/weirdnet-relay/releases/download/v$V
    curl -fLO $BASE/weirdnet-relay_$V-1_$ARCH.deb -O $BASE/SHA256SUMS
    sha256sum --check --ignore-missing SHA256SUMS
    sudo apt-get install ./weirdnet-relay_$V-1_$ARCH.deb

Until the first release is out, build the same package from source:

    sudo apt-get install -y build-essential pkg-config perl git curl dpkg-dev
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
    cargo install cargo-deb --locked
    git clone https://github.com/WeirdBrains/weirdnet-relay && cd weirdnet-relay
    cargo deb --locked -o dist/
    sudo apt-get install ./dist/weirdnet-relay_*.deb

The package installs:

- `/usr/bin/weirdnet-relay`
- `/usr/lib/systemd/system/weirdnet-relay.service`: the hardened unit (`DynamicUser=yes`,
  state in `/var/lib/weirdnet-relay`, made by systemd on first start)
- `/etc/weirdnet-relay/relay.toml`: a starting config, kept across upgrades
- `/usr/share/doc/weirdnet-relay/`: this guide, the README and the full example config

It does **not** start the relay: the unit only starts once `/etc/weirdnet-relay/db-key.cred`
exists (step 3). SQLCipher and OpenSSL are built into the binary; no other libraries are needed.

## 2. Configure

Edit `/etc/weirdnet-relay/relay.toml`:

```toml
name = "WeirdBrains public relay"
operator = "WeirdBrains"
contact = "<ABUSE_CONTACT>"     # an address that reads abuse reports
description = "The WeirdBrains public relay and bootstrap node for Weirdnet."

[network]
port = 4747
external = ["/dns4/relay.net.weirdbrains.com/tcp/4747", "/dns4/relay.net.weirdbrains.com/udp/4747/quic-v1"]
bootstrap = []

[limits]
preset = "unlimited"

[http]
listen = "127.0.0.1:4780"
```

Every key is listed in `/usr/share/doc/weirdnet-relay/examples/relay.toml`.

## 3. The database key

The relay's node key (its peer id) and its state live in an encrypted SQLCipher database.
Make its key once and hand it to systemd as an encrypted credential:

    weirdnet-relay gen-key | sudo systemd-creds encrypt --name=db-key - /etc/weirdnet-relay/db-key.cred
    sudo chmod 0600 /etc/weirdnet-relay/db-key.cred

`systemd-creds` ties the file to this host (its TPM if it has one, else
`/var/lib/systemd/credential.secret`). Keep a copy of the key itself somewhere offline:
without it, a rebuilt server gets a new peer id and every app's bootstrap entry breaks. To
keep such a copy, run `weirdnet-relay gen-key > db-key` on a trusted machine first, encrypt
from that file (`systemd-creds encrypt --name=db-key db-key /etc/weirdnet-relay/db-key.cred`),
store `db-key` offline and delete it from the server.

Check the config and the key the way the service will see them:

    sudo systemd-run --pipe --wait -p DynamicUser=yes -p StateDirectory=weirdnet-relay \
        -p LoadCredentialEncrypted=db-key:/etc/weirdnet-relay/db-key.cred \
        /usr/bin/weirdnet-relay --config /etc/weirdnet-relay/relay.toml check-config

## 4. Start it

    sudo systemctl enable --now weirdnet-relay
    journalctl -u weirdnet-relay -f

The log's `relay started peer_id=…` line gives the peer id. So does
`curl -s 127.0.0.1:4780/healthz`.

## 5. Firewall

Open TCP and UDP 4747 (TCP and QUIC), and keep 4780 local:

    sudo ufw allow 4747/tcp
    sudo ufw allow 4747/udp

If the provider has its own firewall (security groups, cloud firewall), open the same
ports there. To publish the info page as https://relay.net.weirdbrains.com/, put a TLS
reverse proxy on 80 and 443 in front of `127.0.0.1:4780`, for example Caddy:

    relay.net.weirdbrains.com {
        reverse_proxy 127.0.0.1:4780
    }

## 6. Check from outside

    curl -s https://relay.net.weirdbrains.com/.well-known/weirdnet-relay.json

The `addrs` there are what apps use:

    /dns4/relay.net.weirdbrains.com/tcp/4747/p2p/<RELAY_PEER_ID>
    /dns4/relay.net.weirdbrains.com/udp/4747/quic-v1/p2p/<RELAY_PEER_ID>

Replace `<RELAY_PEER_ID>` in this repository's README and example config, and on
https://net.weirdbrains.com/relay, with the real peer id.

## Updating

Install the new release's .deb the same way (`sudo apt-get install ./weirdnet-relay_….deb`).
The package restarts a running relay; the database and its key carry over and the peer id
stays the same. `/etc/weirdnet-relay/relay.toml` is a conffile: your edits are kept.

## Removing

`sudo apt-get remove weirdnet-relay` stops and disables the service. `purge` also drops
`relay.toml` but keeps `/etc/weirdnet-relay/db-key.cred` and the relay's state
(`/var/lib/private/weirdnet-relay`); delete those yourself only if the peer id should go too.
