//! `weirdnet-relay`: run a Weirdnet relay, print its id or info, check a config, make a key.
use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;
use weirdnet_relay::config::{Overrides, Preset};
use weirdnet_relay::store::Store;
use weirdnet_relay::{Config, Relay, dbkey};

#[derive(Parser, Debug)]
#[command(name = "weirdnet-relay", version, about = "A relay and bootstrap node for the Weirdnet network")]
struct Cli {
    /// The TOML config file.
    #[arg(long, short, global = true, env = "WEIRDNET_RELAY_CONFIG")]
    config: Option<PathBuf>,
    #[arg(long, global = true, env = "WEIRDNET_RELAY_NAME")]
    name: Option<String>,
    #[arg(long, global = true, env = "WEIRDNET_RELAY_OPERATOR")]
    operator: Option<String>,
    /// Abuse and contact address shown on the info page.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_CONTACT")]
    contact: Option<String>,
    #[arg(long, global = true, env = "WEIRDNET_RELAY_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// The database key file (64 hex characters or 32 bytes); default: the systemd credential `db-key`.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_DB_KEY_FILE")]
    db_key_file: Option<PathBuf>,
    #[arg(long, global = true, env = "WEIRDNET_RELAY_PORT")]
    port: Option<u16>,
    /// Listen address (repeatable; default: TCP and QUIC on every interface).
    #[arg(long, global = true, env = "WEIRDNET_RELAY_LISTEN", value_delimiter = ',')]
    listen: Vec<String>,
    /// External address others reach this relay at (repeatable), without /p2p/.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_EXTERNAL", value_delimiter = ',')]
    external: Vec<String>,
    /// Bootstrap node or other relay (repeatable), with /p2p/.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_BOOTSTRAP", value_delimiter = ',')]
    bootstrap: Vec<String>,
    #[arg(long, global = true, env = "WEIRDNET_RELAY_MDNS")]
    mdns: Option<bool>,
    /// Where the info page listens (host:port).
    #[arg(long, global = true, env = "WEIRDNET_RELAY_HTTP_LISTEN")]
    http_listen: Option<String>,
    /// Don't serve the info page.
    #[arg(long, global = true)]
    no_http: bool,
    /// Relay limits preset: `unlimited` (default) or `libp2p`.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_LIMITS", value_parser = parse_preset)]
    limits: Option<Preset>,
    /// `pretty` or `json`.
    #[arg(long, global = true, env = "WEIRDNET_RELAY_LOG_FORMAT")]
    log_format: Option<String>,
    /// A tracing filter (`info`, `debug`, `weirdnet_relay=debug,info`).
    #[arg(long, global = true, env = "WEIRDNET_RELAY_LOG")]
    log_level: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the relay (the default).
    Run,
    /// Print the relay's peer id (creating its key on first use).
    Id,
    /// Print the info document the relay would serve.
    Info,
    /// Check the configuration and the database key, then exit.
    CheckConfig,
    /// Print a new random database key (64 hex characters).
    GenKey,
    /// Stop forwarding a site (an abuse report, or the operator's own call); a running relay
    /// picks it up within seconds.
    RevokeSite {
        name: String,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Forward a revoked site again (its host registers again).
    UnrevokeSite { name: String },
    /// List revoked sites.
    RevokedSites,
    /// Show the DNS records `[ddns]` keeps for this relay's public address and what would
    /// change; `--apply` writes them.
    Ddns {
        #[arg(long)]
        apply: bool,
    },
}

fn parse_preset(s: &str) -> Result<Preset, String> {
    match s {
        "unlimited" => Ok(Preset::Unlimited),
        "libp2p" => Ok(Preset::Libp2p),
        _ => Err("`unlimited` or `libp2p`".into()),
    }
}

fn config(cli: &Cli) -> anyhow::Result<Config> {
    let mut cfg = match &cli.config {
        Some(p) => Config::from_file(p)?,
        None => Config::default(),
    };
    cfg.apply(&Overrides {
        name: cli.name.clone(),
        operator: cli.operator.clone(),
        contact: cli.contact.clone(),
        state_dir: cli.state_dir.clone(),
        db_key_file: cli.db_key_file.clone(),
        port: cli.port,
        listen: cli.listen.clone(),
        external: cli.external.clone(),
        bootstrap: cli.bootstrap.clone(),
        mdns: cli.mdns,
        http_listen: cli.http_listen.clone(),
        no_http: cli.no_http,
        limits: cli.limits,
        log_format: cli.log_format.clone(),
        log_level: cli.log_level.clone(),
    });
    cfg.validate()?;
    Ok(cfg)
}

fn logging(cfg: &Config) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_new(&cfg.log.level).unwrap_or_else(|_| EnvFilter::new("info"));
    let journald = std::env::var_os("JOURNAL_STREAM").is_some();
    let b = tracing_subscriber::fmt().with_env_filter(filter).with_target(true);
    match (cfg.log.format.as_str(), journald) {
        ("json", _) => b.json().init(),
        (_, true) => b.without_time().with_ansi(false).init(),
        _ => b.init(),
    }
}

fn site_name(n: &str) -> anyhow::Result<String> {
    weirdnet_net::ingress::sni::host_name(n.as_bytes()).ok_or_else(|| anyhow::anyhow!("`{n}` is not a host name"))
}

fn open_store(cfg: &Config) -> anyhow::Result<Store> {
    let key_path = dbkey::locate(cfg.db_key_file.as_deref())?;
    let key = dbkey::read(&key_path)?;
    let db = cfg.db_path();
    Store::open(&db, &key).with_context(|| format!("opening {}", db.display()))
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if matches!(cli.cmd, Some(Cmd::GenKey)) {
        println!("{}", dbkey::generate_hex()?.as_str());
        return Ok(());
    }
    let cfg = config(&cli)?;
    logging(&cfg);
    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::GenKey => unreachable!(),
        Cmd::CheckConfig => {
            let store = open_store(&cfg)?;
            if cfg.ddns.enabled {
                weirdnet_relay::ddns::provider(&cfg.ddns)?;
                println!("ddns: {} at {}, token read", cfg.ddns.fqdn(), cfg.ddns.provider);
            }
            println!("config ok; database {} at schema {}", cfg.db_path().display(), store.schema_version()?);
            Ok(())
        }
        Cmd::Id => {
            let store = open_store(&cfg)?;
            let key = weirdnet_net::identity::load_or_create(&store)?;
            println!("{}", key.public().to_peer_id());
            Ok(())
        }
        Cmd::Info => {
            let store = open_store(&cfg)?;
            let relay = Relay::start_opts(cfg, store, false)?;
            std::thread::sleep(Duration::from_millis(500));
            println!("{}", relay.refresh().to_json());
            relay.stop();
            Ok(())
        }
        Cmd::RevokeSite { name, reason } => {
            let n = site_name(&name)?;
            open_store(&cfg)?.revoke_site(&n, &reason, weirdnet_net::now_ms())?;
            println!("revoked {n}");
            Ok(())
        }
        Cmd::UnrevokeSite { name } => {
            let n = site_name(&name)?;
            if open_store(&cfg)?.unrevoke_site(&n)? {
                println!("{n} is forwarded again once its host registers");
            } else {
                println!("{n} was not revoked");
            }
            Ok(())
        }
        Cmd::RevokedSites => {
            for r in open_store(&cfg)?.revoked_sites()? {
                println!("{}\t{}\t{}", r.name, r.revoked_at, r.reason);
            }
            Ok(())
        }
        Cmd::Ddns { apply } => ddns(cfg, apply),
        Cmd::Run => run(cfg),
    }
}

fn ddns(cfg: Config, apply: bool) -> anyhow::Result<()> {
    use weirdnet_relay::ddns;
    if !cfg.ddns.enabled {
        anyhow::bail!("[ddns] is off; set enabled = true with a provider, zone and name");
    }
    let store = open_store(&cfg)?;
    let peer = weirdnet_net::identity::load_or_create(&store)?.public().to_peer_id();
    let dns = ddns::provider(&cfg.ddns)?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let plan = rt.block_on(async {
        let found = grindvakt_core::ip::discover(ddns::family(&cfg.ddns), &grindvakt_core::ip::Sources::default()).await?;
        let want = ddns::wanted(&cfg, &peer, &found)?;
        anyhow::Ok(ddns::sync(&dns, &cfg.ddns.zone, &want, !apply).await?)
    })?;
    print!("{plan}");
    if !apply && !plan.is_noop() {
        println!("(dry run; pass --apply to write)");
    }
    Ok(())
}

fn run(cfg: Config) -> anyhow::Result<()> {
    let store = open_store(&cfg)?;
    let http = if cfg.http.enabled { Some(cfg.http_addr()?) } else { None };
    let relay = Relay::start(cfg, store)?;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("weirdnet-relay-http").build()?;
    rt.block_on(async {
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let server = match http {
            Some(addr) => {
                let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("info page on {addr}"))?;
                tracing::info!("info page on http://{addr}/");
                let app = weirdnet_relay::http::router(relay.info_handle());
                let mut rx = stop_rx.clone();
                Some(tokio::spawn(async move {
                    let _ = axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = rx.wait_for(|s| *s).await;
                        })
                        .await;
                }))
            }
            None => None,
        };
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut prune = tokio::time::interval(Duration::from_secs(24 * 3600));
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    relay.refresh();
                    if let Err(e) = relay.sync_revoked() {
                        tracing::warn!("revoked sites: {e}");
                    }
                    if !relay.running() {
                        tracing::error!("the node stopped");
                        break;
                    }
                }
                _ = prune.tick() => {
                    let month = weirdnet_net::now_ms() - 30 * 24 * 3600 * 1000;
                    match relay.store().prune_peers(month) {
                        Ok(n) if n > 0 => tracing::info!("forgot {n} peers not seen for 30 days"),
                        Ok(_) => {}
                        Err(e) => tracing::warn!("pruning peers: {e}"),
                    }
                }
                _ = tokio::signal::ctrl_c() => break,
                _ = term.recv() => break,
            }
        }
        tracing::info!("stopping");
        let _ = stop_tx.send(true);
        if let Some(s) = server {
            let _ = s.await;
        }
        anyhow::Ok(())
    })?;
    relay.stop();
    Ok(())
}
