# Deploying the WeirdBrains public relay

The WeirdBrains public relay is **relay.net.weirdbrains.com**.
These steps install weirdnet-relay there with systemd. They work the same on any Linux
server with systemd 250 or newer (for `systemd-creds`).

Status: **live** since 2026-11-16 (https://relay.net.weirdbrains.com/). Peer id
`12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB`. Runs 0.6.0 (weirdnet-net 0.8) since 2026-11-19.

## 1. Install the package

Each release on https://github.com/WeirdBrains/weirdnet-relay/releases carries packages for
x86_64 and aarch64, a `SHA256SUMS` file and, when signed, `SHA256SUMS.minisig`. They all need
systemd 250 or newer (for `systemd-creds`). The `.rpm` and Arch packages start with 0.1.1.

Fetch and check a file (set `V` and the file name from the list below):

    V=$(curl -fsSL https://api.github.com/repos/WeirdBrains/weirdnet-relay/releases/latest | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p')
    BASE=https://github.com/WeirdBrains/weirdnet-relay/releases/download/v$V
    curl -fLO $BASE/FILE -O $BASE/SHA256SUMS
    sha256sum --check --ignore-missing SHA256SUMS

**Debian 12+, Ubuntu 24.04+ (apt).** `weirdnet-relay_$V-1_amd64.deb` or `_arm64.deb`:

    sudo apt-get install ./weirdnet-relay_$V-1_$(dpkg --print-architecture).deb

**Fedora, RHEL 9+ and clones (dnf); openSUSE (zypper).** `weirdnet-relay-$V-1.x86_64.rpm`
or `.aarch64.rpm`. The packages are not GPG-signed; check `SHA256SUMS` (and its minisig) first:

    sudo dnf install ./weirdnet-relay-$V-1.$(uname -m).rpm
    sudo zypper install --allow-unsigned-rpm ./weirdnet-relay-$V-1.$(uname -m).rpm

**Arch Linux, Omarchy (pacman).** From TAUR, the WeirdBrains Arch user repository
(https://pkg.taur.weirdbrains.com/packages/weirdnet-relay), with an AUR helper (not as root):

    paru --aururl https://pkg.taur.weirdbrains.com --aurrpcurl https://pkg.taur.weirdbrains.com/rpc -S weirdnet-relay
    yay --aururl https://pkg.taur.weirdbrains.com --aurrpcurl 'https://pkg.taur.weirdbrains.com/rpc?' -S weirdnet-relay

or by hand (`git clone https://pkg.taur.weirdbrains.com/weirdnet-relay.git && cd weirdnet-relay
&& makepkg -si`), or install the release's built package directly:

    sudo pacman -U ./weirdnet-relay-$V-1-$(uname -m).pkg.tar.zst

The recipe installs the prebuilt binary from the release tarball; nothing is compiled.

**Other Linux.** `weirdnet-relay-$V-x86_64-linux.tar.gz` or `-aarch64-linux.tar.gz` has the
binary, `systemd/weirdnet-relay.service`, the packaged `relay.toml` and `examples/relay.toml`.
Copy them to the paths below.

**From source** (Debian/Ubuntu), the same .deb:

    sudo apt-get install -y build-essential pkg-config perl git curl dpkg-dev
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
    cargo install cargo-deb --locked
    git clone https://github.com/WeirdBrains/weirdnet-relay && cd weirdnet-relay
    cargo deb --locked -o dist/
    sudo apt-get install ./dist/weirdnet-relay_*.deb

Every package installs the same files:

- `/usr/bin/weirdnet-relay`
- `/usr/lib/systemd/system/weirdnet-relay.service`: the hardened unit (`DynamicUser=yes`,
  state in `/var/lib/weirdnet-relay`, made by systemd on first start)
- `/etc/weirdnet-relay/relay.toml`: a starting config, kept across upgrades (a conffile,
  `%config(noreplace)` or pacman `backup`)
- `/usr/share/doc/weirdnet-relay/`: this guide, the README and the full example config

It does **not** start the relay: the unit only starts once `/etc/weirdnet-relay/db-key.cred`
exists (step 3). SQLCipher and OpenSSL are built into the binary; no other libraries are needed.

## 2. Configure

Edit `/etc/weirdnet-relay/relay.toml`:

```toml
name = "WeirdBrains public relay"
operator = "WeirdBrains"
contact = "michael+relay@weirdbrains.com"     # an address that reads abuse reports
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

    /dns4/relay.net.weirdbrains.com/tcp/4747/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB
    /dns4/relay.net.weirdbrains.com/udp/4747/quic-v1/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB

These are in this repository's README and example config and on
https://net.weirdbrains.com/relay. A rebuilt server with a new key changes the peer id; update
them all then.

## 7. WebSocket over TLS and site forwarding (optional)

**WebSocket over TLS** lets apps on networks that only let HTTPS out reach the relay at
`/dns4/relay.net.weirdbrains.com/tcp/443/tls/ws/p2p/<id>`. With the info page already behind
Caddy on 443, let Caddy keep the certificate and hand WebSocket upgrades to the relay:

```toml
[wss]
enabled = true
name = "relay.net.weirdbrains.com"
port = 443          # what apps dial (Caddy's)
local_port = 4743   # plain WebSocket on 127.0.0.1, from Caddy
```

    relay.net.weirdbrains.com {
        @ws {
            header Connection *Upgrade*
            header Upgrade websocket
        }
        reverse_proxy @ws 127.0.0.1:4743
        reverse_proxy 127.0.0.1:4780
    }

`network.external` must be set (the relay then announces exactly those addresses and the WSS one).

**Site forwarding** gives Weirdnet hosts without a public address public HTTPS: the relay
takes 443 (and 80), reads only the TLS server name, and passes the connection to the host that
registered that name with its owner's signed grant and a `_weirdnet.<name>` TXT binding. The
relay never holds a site's key or sees its traffic. Port 80 passes ACME HTTP-01 for those
names and redirects everything else to HTTPS. Caddy moves behind it to a local port, and its
own name is passed through untouched:

```toml
[forwarding]
enabled = true
https = "[::]:443"
http = "[::]:80"
local = { "relay.net.weirdbrains.com" = "127.0.0.1:8443" }
```

and in the Caddyfile, `https_port 8443`, `auto_https disable_redirects`, and the site as
`relay.net.weirdbrains.com:8443` (Caddy then gets its certificate by TLS-ALPN-01 through the
passthrough). Binding 80 and 443 needs a drop-in, because the unit runs with no capabilities:

    sudo systemctl edit weirdnet-relay

```ini
[Service]
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_BIND_SERVICE
PrivateUsers=no
```

Open TCP 80 and 443 in the firewall. To stop forwarding a site (an abuse report to
`contact`, or the operator's own call):

    sudo weirdnet-relay --config /etc/weirdnet-relay/relay.toml revoke-site site.example.com --reason "abuse report"
    sudo weirdnet-relay --config /etc/weirdnet-relay/relay.toml revoked-sites
    sudo weirdnet-relay --config /etc/weirdnet-relay/relay.toml unrevoke-site site.example.com

(run them the way `check-config` runs in step 3, so they open the same database). A running
relay picks the change up within seconds. `enabled = false` turns forwarding off entirely.

## 8. Dynamic DNS (optional)

For a relay on a home connection or an address that changes, `[ddns]` keeps its name
current through Grindvakt. Put the DNS provider's API token (DigitalOcean: a token with
domain write access; DNSimple: an account token) in an encrypted credential:

    sudo systemd-creds encrypt --name=dns-token - /etc/weirdnet-relay/dns-token.cred   # paste the token, then Ctrl-D
    sudo chmod 0600 /etc/weirdnet-relay/dns-token.cred
    sudo systemctl edit weirdnet-relay   # add, under [Service]:
    #   LoadCredentialEncrypted=dns-token:/etc/weirdnet-relay/dns-token.cred

Set `[ddns]` in relay.toml (`enabled = true`, `provider`, `zone`, `name`), list the name
under `[network] external`, then check and preview it the way `check-config` runs in step 3,
with `-p LoadCredentialEncrypted=dns-token:/etc/weirdnet-relay/dns-token.cred` added and
`ddns` instead of `check-config`. Restart the relay; it logs each record it writes.

## Updating

Install the new release's package the same way (`apt-get install`, `dnf install`,
`zypper install`, `pacman -U`, or `paru -Syu` / `yay -Syu` with the TAUR flags above). The
package restarts a running relay; the database and its key carry over and the peer id stays
the same. Your edits to `/etc/weirdnet-relay/relay.toml` are kept (pacman and rpm write the
new default next to it as `relay.toml.pacnew` or `relay.toml.rpmnew` when both changed).

## Removing

`sudo apt-get remove weirdnet-relay` (`dnf remove`, `zypper remove`, `pacman -R`) stops and
disables the service. apt's `purge` also drops `relay.toml`; rpm and pacman keep a changed one
as `relay.toml.rpmsave` or `relay.toml.pacsave`. All of them keep `/etc/weirdnet-relay/db-key.cred` and the relay's state
(`/var/lib/private/weirdnet-relay`); delete those yourself only if the peer id should go too.
