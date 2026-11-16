# Deploying the WeirdBrains public relay

The WeirdBrains public relay is **relay.net.weirdbrains.com**.
These steps install weirdnet-relay there with systemd. They work the same on any Linux
server with systemd 250 or newer (for `systemd-creds`).

Status: **coming online.** The DNS name is live; the relay is not deployed yet. Its peer id
is filled in below once the relay has made its key (`<RELAY_PEER_ID>` until then).

## 1. Build

    sudo apt-get install -y build-essential pkg-config perl git curl   # Debian/Ubuntu
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
    git clone https://github.com/WeirdBrains/weirdnet-relay
    cd weirdnet-relay
    cargo build --release --locked
    sudo install -m 0755 target/release/weirdnet-relay /usr/bin/

SQLCipher and OpenSSL are vendored and built from source; no system libraries are needed
at run time.

## 2. Configure

    sudo install -d -m 0755 /etc/weirdnet-relay
    sudo install -m 0644 examples/relay.toml /etc/weirdnet-relay/relay.toml

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

## 4. systemd

    sudo install -m 0644 packaging/systemd/weirdnet-relay.service /etc/systemd/system/
    sudo systemctl daemon-reload
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

    cd weirdnet-relay && git pull && cargo build --release --locked
    sudo install -m 0755 target/release/weirdnet-relay /usr/bin/ && sudo systemctl restart weirdnet-relay

The database and its key carry over; the peer id stays the same.
