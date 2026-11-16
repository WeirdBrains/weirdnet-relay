# Security

Report vulnerabilities privately to security@weirdbrains.com.

What the relay holds and how:

- **The node key** (its peer id) and **the peer table** live in `relay.db`, a SQLCipher
  database (weirdnet-storage). The file is created 0600 in a 0700 state directory.
- **The database key** is 32 bytes (64 hex characters) read from the systemd credential
  `db-key` (`LoadCredentialEncrypted=`, so it is encrypted at rest with the host's TPM or
  credential secret) or from a key file you name. A key file readable by group or others
  draws a warning. The key is held in zeroizing memory and never logged.
- **What it serves:** the info page and JSON carry only public facts (name, contact, peer id,
  addresses, protocols, limits, NAT status, connected-peer count). Nothing secret is served.
- **What it relays:** with the default `unlimited` preset there are no caps and no rate
  limiters, so one relay can be used for as much traffic as its uplink carries. Circuits are
  end-to-end encrypted (Noise/TLS between the two ends); the relay sees peer ids and byte
  counts, not content. Use `preset = "libp2p"` or per-field limits to cap a public relay.
- **Wakes:** wake payloads are sealed, fixed-size and passed on unread; the relay keeps them
  only in memory, briefly, and caps them per handle. A push token file is read once and never
  logged.
- **Update announcements:** the relay stores only announcements signed for the orgs in
  `[updates] roots`, the latest per org, app and channel, and refuses forged, mis-scoped or
  expired ones. It can't make an update acceptable: devices check every announcement against
  their own root and verify the update itself. A relay can only withhold news.
