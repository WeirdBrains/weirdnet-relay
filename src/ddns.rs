//! Dynamic DNS through Grindvakt ([grindvakt-core](https://crates.io/crates/grindvakt-core)).
//!
//! A relay you run yourself needs a name tied to the machine (see the collective's relay
//! notes). With `[ddns]` on, the relay looks up its public IPv4 and IPv6 addresses every
//! `interval_secs` and keeps `A`/`AAAA` records for its name current at DigitalOcean or
//! DNSimple, plus `_dnsaddr.<name>` TXT records (`dnsaddr=/dns/<name>/tcp/<port>/p2p/<peer>`
//! and the QUIC and WSS forms) so peers can dial `/dnsaddr/<name>`.
//!
//! Grindvakt never deletes records and refuses to guess: when the zone holds more records
//! at a name than the relay wants (two `A` records, say), nothing is written and the relay
//! says why. The token is read once from a file (by default the systemd credential
//! `dns-token`), kept as a secret and never logged.
use crate::config::{Config, Ddns as DdnsConfig};
use crate::error::{Error, Result};
use grindvakt_core::config::Provider;
use grindvakt_core::ip::{self, Discovered, Family, Sources};
use grindvakt_core::plan::Plan;
use grindvakt_core::provider::{AnyProvider, DnsProvider, ProviderOptions};
use grindvakt_core::record::Record;
use grindvakt_core::{ExposeSecret, SecretString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use weirdnet_net::PeerId;

/// The systemd credential the token is read from by default.
pub const CREDENTIAL: &str = "dns-token";
/// How often records are planned again even when the address hasn't changed, to repair
/// edits made by hand.
const RECHECK: Duration = Duration::from_secs(6 * 3600);

/// The address families `cfg` keeps.
pub fn family(cfg: &DdnsConfig) -> Family {
    match (cfg.ipv4, cfg.ipv6) {
        (true, false) => Family::V4,
        (false, true) => Family::V6,
        _ => Family::Both,
    }
}

/// The ports the relay listens on: TCP ones, then QUIC ones.
fn ports(cfg: &Config) -> Result<(Vec<u16>, Vec<u16>)> {
    let (mut tcp, mut quic) = (Vec::new(), Vec::new());
    for a in cfg.listen_addrs()? {
        let s = a.to_string();
        let parts: Vec<&str> = s.split('/').collect();
        let port = |proto: &str| parts.windows(2).find(|w| w[0] == proto).and_then(|w| w[1].parse::<u16>().ok());
        if s.contains("/ws") || s.contains("/tls") {
            continue;
        }
        if s.contains("/quic") {
            if let Some(p) = port("udp").filter(|p| !quic.contains(p)) {
                quic.push(p);
            }
        } else if let Some(p) = port("tcp").filter(|p| !tcp.contains(p)) {
            tcp.push(p);
        }
    }
    Ok((tcp, quic))
}

/// The records the relay wants for the addresses `found`.
pub fn wanted(cfg: &Config, peer: &PeerId, found: &Discovered) -> Result<Vec<Record>> {
    let d = &cfg.ddns;
    let fqdn = d.fqdn();
    let mut want = Vec::new();
    if let Some(v4) = found.v4.filter(|_| d.ipv4) {
        want.push(Record::a(fqdn.clone(), v4).with_ttl(d.ttl));
    }
    if let Some(v6) = found.v6.filter(|_| d.ipv6) {
        want.push(Record::aaaa(fqdn.clone(), v6).with_ttl(d.ttl));
    }
    if want.is_empty() {
        return Err(Error::Ddns("no public address to publish".into()));
    }
    if d.dnsaddr {
        let (tcp, quic) = ports(cfg)?;
        let mut addrs: Vec<String> = tcp.iter().map(|p| format!("/dns/{fqdn}/tcp/{p}/p2p/{peer}")).collect();
        addrs.extend(quic.iter().map(|p| format!("/dns/{fqdn}/udp/{p}/quic-v1/p2p/{peer}")));
        if cfg.wss.enabled && cfg.wss.name.trim().trim_end_matches('.').eq_ignore_ascii_case(&fqdn) {
            addrs.push(format!("/dns/{fqdn}/tcp/{}/tls/ws/p2p/{peer}", cfg.wss.port));
        }
        let name = weirdnet_net::addr::dnsaddr_name(&fqdn);
        for a in addrs {
            let m = a.parse().map_err(|e| Error::Ddns(format!("{a}: {e}")))?;
            let txt = weirdnet_net::addr::dnsaddr_txt(&m)?;
            want.push(Record::txt(name.clone(), txt).with_ttl(d.ttl));
        }
    }
    Ok(want)
}

fn dd(e: grindvakt_core::Error) -> Error {
    Error::Ddns(e.to_string())
}

/// Plans `want` against the zone and, unless `dry_run`, writes the changes. A plan with any
/// ambiguous record is refused whole: nothing is written.
pub async fn sync<P: DnsProvider + Sync + ?Sized>(dns: &P, zone: &str, want: &[Record], dry_run: bool) -> Result<Plan> {
    let plan = grindvakt_core::plan(dns, zone, want).await.map_err(dd)?;
    if let Some(a) = plan.ambiguous().next() {
        return Err(Error::Ddns(format!("refusing to change records: {a}")));
    }
    if !dry_run && !plan.is_noop() {
        grindvakt_core::apply(dns, &plan).await.map_err(dd)?;
    }
    Ok(plan)
}

/// Where the token is read from: `token_file`, else the systemd credential `dns-token`.
pub fn token_path(cfg: &DdnsConfig) -> Result<PathBuf> {
    if let Some(p) = &cfg.token_file {
        return Ok(p.clone());
    }
    if let Some(dir) = std::env::var_os("CREDENTIALS_DIRECTORY") {
        let p = Path::new(&dir).join(CREDENTIAL);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(Error::Config("ddns: set token_file, or load the systemd credential `dns-token` (LoadCredentialEncrypted=dns-token:…)".into()))
}

/// Reads the token (one line). It is never logged.
pub fn read_token(path: &Path) -> Result<SecretString> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path)
            && m.permissions().mode() & 0o077 != 0
        {
            tracing::warn!("the DNS token file {} is readable by others; chmod 0600 it", path.display());
        }
    }
    let t = std::fs::read_to_string(path).map_err(|e| Error::Config(format!("ddns token file {}: {e}", path.display())))?;
    let t = SecretString::from(t.trim().to_string());
    if t.expose_secret().is_empty() {
        return Err(Error::Config(format!("ddns token file {} is empty", path.display())));
    }
    Ok(t)
}

/// The configured provider, with its token.
pub fn provider(cfg: &DdnsConfig) -> Result<AnyProvider> {
    let kind = match cfg.provider.as_str() {
        "digitalocean" => Provider::Digitalocean,
        "dnsimple" => Provider::Dnsimple,
        other => return Err(Error::Config(format!("ddns.provider `{other}`"))),
    };
    let token = read_token(&token_path(cfg)?)?;
    let opts = ProviderOptions { base_url: (!cfg.api.is_empty()).then(|| cfg.api.clone()), account_id: cfg.dnsimple_account, ..Default::default() };
    AnyProvider::new(kind, &token, &opts).map_err(dd)
}

/// What the updater last saw and did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// The addresses last published.
    pub published: Option<Discovered>,
    /// When records were last checked or written without an error (Unix ms).
    pub last_ok_ms: Option<i64>,
    /// Records written since the relay started.
    pub changes: u64,
    /// The last error, cleared by the next success.
    pub last_error: Option<String>,
}

/// One round: look up the addresses with `discover`, and plan (and write) when they changed
/// or `recheck` is due. Returns how long to wait before the next round.
pub async fn round<P, F, Fut>(dns: &P, cfg: &Config, peer: &PeerId, status: &Mutex<Status>, recheck: bool, discover: F) -> Duration
where
    P: DnsProvider + Sync + ?Sized,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = grindvakt_core::Result<Discovered>>,
{
    let interval = Duration::from_secs(cfg.ddns.interval_secs);
    let set_err = |e: String| {
        let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
        if s.last_error.as_deref() != Some(&e) {
            tracing::warn!("ddns: {e}");
        }
        s.last_error = Some(e);
    };
    let found = match discover().await {
        Ok(f) => f,
        Err(grindvakt_core::Error::NoGlobalAddress(why)) => {
            set_err(format!("no public address to publish ({why}); behind CGNAT, the relay needs a public IP"));
            return interval;
        }
        Err(e) => {
            set_err(format!("looking up the public address: {e}"));
            return interval;
        }
    };
    let published = status.lock().unwrap_or_else(|p| p.into_inner()).published;
    if published == Some(found) && !recheck {
        return interval;
    }
    let want = match wanted(cfg, peer, &found) {
        Ok(w) => w,
        Err(e) => {
            set_err(e.to_string());
            return interval;
        }
    };
    match sync(dns, &cfg.ddns.zone, &want, false).await {
        Ok(plan) => {
            let n = plan.changes().count() as u64;
            for a in plan.changes() {
                tracing::info!("ddns: {a}");
            }
            let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
            s.published = Some(found);
            s.last_ok_ms = Some(weirdnet_net::now_ms());
            s.changes += n;
            s.last_error = None;
            interval
        }
        Err(e) => {
            let wait = match &e {
                Error::Ddns(m) if m.contains("rate limited") => interval.max(Duration::from_secs(900)),
                _ => interval,
            };
            set_err(e.to_string());
            wait
        }
    }
}

/// The running updater.
pub struct DdnsService {
    status: Arc<Mutex<Status>>,
    rt: Option<tokio::runtime::Runtime>,
}

impl DdnsService {
    /// Reads the token and starts updating the relay's records.
    pub fn start(cfg: &Config, peer: PeerId) -> Result<Self> {
        let dns = provider(&cfg.ddns)?;
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().thread_name("weirdnet-relay-ddns").build()?;
        let status = Arc::new(Mutex::new(Status::default()));
        let (cfg, st) = (cfg.clone(), status.clone());
        tracing::info!(name = %cfg.ddns.fqdn(), provider = %cfg.ddns.provider, "dynamic DNS through Grindvakt");
        rt.spawn(async move {
            let fam = family(&cfg.ddns);
            let sources = Sources::default();
            let mut last_check: Option<Instant> = None;
            loop {
                let recheck = last_check.is_none_or(|t| t.elapsed() > RECHECK);
                let wait = round(&dns, &cfg, &peer, &st, recheck, || ip::discover(fam, &sources)).await;
                if st.lock().unwrap_or_else(|p| p.into_inner()).last_error.is_none() && recheck {
                    last_check = Some(Instant::now());
                }
                tokio::time::sleep(wait).await;
            }
        });
        Ok(Self { status, rt: Some(rt) })
    }

    /// What the updater last saw and did.
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl Drop for DdnsService {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}
