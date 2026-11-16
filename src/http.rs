//! The info page: `GET /` (HTML for people), `GET /.well-known/weirdnet-relay.json`
//! (for apps, with CORS open) and `GET /healthz`. Nothing secret is ever served.
use crate::info::{RelayInfo, WELL_KNOWN_PATH};
use crate::relay::InfoHandle;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

const MARK: &str = include_str!("../assets/weirdnet-mark.svg");

/// The routes, over the shared info document.
pub fn router(info: InfoHandle) -> Router {
    Router::new()
        .route("/", get(page))
        .route(WELL_KNOWN_PATH, get(json))
        .route("/healthz", get(health))
        .fallback(|| async { (StatusCode::NOT_FOUND, "not found\n") })
        .with_state(info)
}

async fn json(State(info): State<InfoHandle>) -> Response {
    let mut r = (StatusCode::OK, info.get().to_json()).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

async fn health(State(info): State<InfoHandle>) -> Response {
    let i = info.get();
    let body = serde_json::json!({ "ok": true, "peer_id": i.peer_id, "nat": i.status.nat, "connected_peers": i.status.connected_peers });
    let mut r = (StatusCode::OK, body.to_string()).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    r
}

async fn page(State(info): State<InfoHandle>) -> Html<String> {
    Html(render(&info.get()))
}

/// Escape text for HTML.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

fn list(items: &[String]) -> String {
    items.iter().map(|i| format!("<li><code>{}</code></li>", esc(i))).collect()
}

fn limit(v: Option<u64>, unit: &str) -> String {
    v.map(|n| format!("{n}{unit}")).unwrap_or_else(|| "no limit".into())
}

/// The HTML page.
pub fn render(i: &RelayInfo) -> String {
    let contact = if i.contact.is_empty() {
        String::new()
    } else {
        let c = esc(&i.contact);
        let href = if i.contact.contains('@') && !i.contact.contains(':') { format!("mailto:{c}") } else { c.clone() };
        format!("<p class=\"meta\">Contact and abuse reports: <a href=\"{href}\">{c}</a></p>")
    };
    let operator = if i.operator.is_empty() { String::new() } else { format!("<p class=\"meta\">Run by {}</p>", esc(&i.operator)) };
    let description = if i.description.is_empty() { String::new() } else { format!("<p class=\"lede\">{}</p>", esc(&i.description)) };
    let first = i.addrs.first().cloned().unwrap_or_default();
    let l = &i.limits;
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{name} · Weirdnet relay</title>
<link rel="alternate" type="application/json" href="{well_known}">
<style>
:root{{--bg:#001b3d;--card:#00224d;--border:#00316b;--muted:#b8bec7;--y:#facc15;--p:#a855f7;--k:#ec4899}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:#fff;font:16px/1.6 Inter,ui-sans-serif,system-ui,sans-serif}}
main{{max-width:60rem;margin:0 auto;padding:3rem 1.25rem}}header{{display:flex;gap:1.2rem;align-items:center}}header svg{{width:4.5rem;height:4.5rem}}
h1{{margin:0;font-size:2.2rem;background:linear-gradient(90deg,var(--y),var(--p),var(--y));-webkit-background-clip:text;background-clip:text;color:transparent}}
h2{{font-size:1.15rem;color:var(--y);margin:2.2rem 0 .6rem}}.meta,.lede{{color:var(--muted);margin:.3rem 0}}
a{{color:#c084fc}}code{{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.88em;overflow-wrap:anywhere}}
ul{{padding-left:1.2rem}}li{{margin:.25rem 0}}.card{{background:var(--card);border:1px solid var(--border);border-radius:.75rem;padding:1rem 1.2rem}}
table{{border-collapse:collapse;width:100%}}td{{border-bottom:1px solid var(--border);padding:.4rem .5rem;vertical-align:top}}td:first-child{{color:var(--muted);width:16rem}}
pre{{background:#00122b;border:1px solid var(--border);border-radius:.5rem;padding:.9rem 1rem;overflow-x:auto}}
footer{{margin-top:3rem;color:var(--muted);font-size:.9rem}}
</style></head><body><main>
<header>{mark}<div><h1>{name}</h1><p class="meta">A Weirdnet relay · <code>{software}</code></p></div></header>
{description}{operator}{contact}
<h2>Peer id</h2><div class="card"><code>{peer}</code></div>
<h2>Addresses</h2><div class="card"><ul>{addrs}</ul></div>
<h2>Add this relay</h2>
<p class="meta">Put an address in your node's <code>relays</code> and <code>bootstrap</code>, or let your app read <a href="{well_known}"><code>{well_known}</code></a>.</p>
<pre><code>relays = ["{first}"]
bootstrap = ["{first}"]</code></pre>
<h2>What it does</h2><div class="card"><table>
<tr><td>Roles</td><td>{roles}</td></tr>
<tr><td>Transports</td><td>{transports}</td></tr>
<tr><td>Network</td><td><code>{kad}</code> · <code>{identify}</code> · relays under <code>{relay_key}</code></td></tr>
<tr><td>Circuits</td><td>{circuits} at once, {circuit_time} each, {circuit_bytes} per direction</td></tr>
<tr><td>Reservations</td><td>{reservations} at once, renewed every {res_secs} s</td></tr>
<tr><td>Rate limits</td><td>{rate}</td></tr>
<tr><td>NAT</td><td>{nat} · {connected} peers connected</td></tr>
</table></div>
<h2>Protocols</h2><div class="card"><ul>{protocols}</ul></div>
<footer>Weirdnet · <a href="https://net.weirdbrains.com/relay">Run your own relay</a> · <a href="https://github.com/WeirdBrains/weirdnet-relay">Source</a></footer>
</main></body></html>
"#,
        name = esc(&i.name),
        well_known = WELL_KNOWN_PATH,
        mark = MARK,
        software = esc(&i.software),
        description = description,
        operator = operator,
        contact = contact,
        peer = esc(&i.peer_id),
        addrs = if i.addrs.is_empty() { "<li>none yet</li>".into() } else { list(&i.addrs) },
        first = esc(&first),
        roles = esc(&i.roles.join(", ")),
        transports = esc(&i.transports.join(", ")),
        kad = esc(&i.network.kad),
        identify = esc(&i.network.identify),
        relay_key = esc(&i.network.relay_key),
        circuits = limit(l.max_circuits, ""),
        circuit_time = limit(l.max_circuit_duration_secs, " s"),
        circuit_bytes = limit(l.max_circuit_bytes, " bytes"),
        reservations = limit(l.max_reservations, ""),
        res_secs = l.reservation_duration_secs,
        rate = if l.rate_limits { "libp2p's per-peer and per-IP limits" } else { "none" },
        nat = esc(&i.status.nat),
        connected = i.status.connected_peers,
        protocols = list(&i.protocols),
    )
}
