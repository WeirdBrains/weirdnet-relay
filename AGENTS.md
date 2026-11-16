# Working on weirdnet-relay

Collective rules apply (straight to main, `git pull --rebase` first, small commits pushed as
they land, no force-push). Before each commit:

    cargo fmt --check && cargo build && cargo clippy --all-targets -- -D warnings && cargo test

- Edition 2024, Rust 1.99+, BSD-3-Clause.
- Secrets never reach logs, output or the repo: not the database key, not the node key.
  `gen-key` is the one command that prints a key, to stdout, for piping into `systemd-creds`.
- The relay is general: anything Weirdnet-wide (behaviours, records, limits) belongs in
  weirdnet-net; this crate is configuration, storage, the info page and the service.
- Docs (README, the example config, the unit, https://net.weirdbrains.com/relay in
  weirdnet-site) change in the same change as the behaviour.
- The wire document (`RelayInfo`, `INFO_VERSION`) only grows; a breaking change bumps `v`.
