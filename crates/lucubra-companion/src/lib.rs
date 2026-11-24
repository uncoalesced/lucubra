// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Local HTTP server bound to the loopback interface, with an embedded
//! static user interface (SPEC section 21, D54).
//!
//! Every request must carry the per-launch token, a `Host` equal to the
//! bound address and, when present, an `Origin` of that address. Anything
//! else gets 403 before any work is done (DNS-rebinding and cross-site
//! protection).
//!
//! shortcut: one connection at a time on one thread, enough for one local
//! user; move to a small async server if the page gains live updates.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use lucubra_core::Workspace;
use lucubra_core::planner::iso_from_days;
use lucubra_core::state::col;
use lucubra_pack::build::{self, FREE_BUDGET, Options, PAID_BUDGET};
use lucubra_pack::ingest::{Outcome, ingest_text};
use serde_json::Value;

/// Largest request head and body accepted.
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 2 * 1024 * 1024;

/// A bound companion server.
pub struct Companion {
    listener: TcpListener,
    addr: SocketAddr,
    token: String,
}

/// A fresh random token: 128 bits drawn from two ULIDs' random parts.
fn new_token() -> String {
    let seed = format!("{}{}", ulid::Ulid::new(), ulid::Ulid::new());
    blake3::hash(seed.as_bytes()).to_hex()[..32].to_owned()
}

impl Companion {
    /// Binds 127.0.0.1 on a free port with a new token.
    pub fn bind() -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        Ok(Self {
            listener,
            addr,
            token: new_token(),
        })
    }

    /// The URL to open, token included.
    pub fn url(&self) -> String {
        format!("http://{}/?t={}", self.addr, self.token)
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Serves requests until the process ends.
    pub fn serve(&self, ws: &Workspace) -> io::Result<()> {
        self.serve_n(ws, usize::MAX)
    }

    /// Serves at most `n` connections.
    pub fn serve_n(&self, ws: &Workspace, n: usize) -> io::Result<()> {
        for stream in self.listener.incoming().take(n) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let response = match read_request(&mut stream) {
                Ok(req) => self.respond(ws, &req),
                Err(status) => page(
                    status,
                    "Bad request",
                    "<p>The request could not be read.</p>",
                ),
            };
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
        Ok(())
    }

    fn allowed(&self, req: &Request) -> bool {
        let bound = self.addr.to_string();
        let host_ok = req.header("host") == Some(bound.as_str());
        let origin_ok = req
            .header("origin")
            .is_none_or(|o| o == format!("http://{bound}"));
        let token = req
            .query("t")
            .or_else(|| req.header("x-lucubra-token"))
            .unwrap_or_default();
        host_ok && origin_ok && constant_eq(token.as_bytes(), self.token.as_bytes())
    }

    fn respond(&self, ws: &Workspace, req: &Request) -> Vec<u8> {
        if !self.allowed(req) {
            return page(
                403,
                "Forbidden",
                "<p>Open the address Lucubra printed when it started.</p>",
            );
        }
        let t = &self.token;
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") => page(200, "Lucubra", &status_body(ws, t)),
            ("POST", "/handoff") => {
                let form = parse_form(&req.body);
                let text = form
                    .iter()
                    .find(|(k, _)| k == "text")
                    .map_or("", |(_, v)| v.as_str());
                page(200, "Handoff", &handoff_body(ws, text, t))
            }
            ("POST", "/pack") => page(200, "Study pack", &pack_body(ws, t)),
            ("POST", "/open-packs") => {
                let dir = ws.root().join("packs");
                let opened = std::fs::create_dir_all(&dir).is_ok() && open_folder(&dir);
                let msg = if opened {
                    "The packs folder is open."
                } else {
                    "Could not open the folder; find it at packs/ in your workspace."
                };
                page(200, "Packs folder", &format!("<p>{msg}</p>{}", back(t)))
            }
            _ => page(
                404,
                "Not found",
                &format!("<p>No such page.</p>{}", back(t)),
            ),
        }
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn open_folder(dir: &std::path::Path) -> bool {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(dir).spawn().is_ok()
}

/// One parsed request.
#[derive(Debug, Default)]
struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, u16> {
    let mut reader = BufReader::new(stream);
    let mut head = Vec::new();
    loop {
        let room = (MAX_HEAD + 1).saturating_sub(head.len()) as u64;
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take(room)
            .read_until(b'\n', &mut line)
            .map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            return Err(431);
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let head = String::from_utf8(head).map_err(|_| 400u16)?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (method, target) = (first.next().ok_or(400u16)?, first.next().ok_or(400u16)?);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut req = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: parse_form(query.as_bytes()),
        headers,
        body: Vec::new(),
    };
    if let Some(len) = req.header("content-length") {
        let len: usize = len.parse().map_err(|_| 400u16)?;
        if len > MAX_BODY {
            return Err(413);
        }
        req.body = vec![0; len];
        reader.read_exact(&mut req.body).map_err(|_| 400u16)?;
    }
    Ok(req)
}

/// `application/x-www-form-urlencoded` pairs.
fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (url_decode(k), url_decode(v))
        })
        .collect()
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(b) = hex {
                    out.push(b);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Graphite, Porcelain and Watermelon from the uncoalesced design language.
const STYLE: &str = "body{background:#333333;color:#FDFFFC;font-family:'IBM Plex Sans',system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem;line-height:1.5}\
h1,h2{font-family:'JetBrains Mono',ui-monospace,monospace;font-weight:600}h1{font-size:1.6rem}\
a{color:#FDFFFC}button{background:#EF2D56;color:#FDFFFC;border:0;border-radius:4px;padding:.5rem 1rem;font:inherit;cursor:pointer}\
button:focus-visible,textarea:focus-visible,a:focus-visible{outline:3px solid #FDFFFC;outline-offset:2px}\
textarea{width:100%;min-height:12rem;background:#262626;color:#FDFFFC;border:1px solid #888;font-family:ui-monospace,monospace}\
.card{border:1px solid #555;border-radius:6px;padding:1rem;margin:1rem 0}";

fn page(status: u16, title: &str, body: &str) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{STYLE}</style></head><body><main>{body}</main></body></html>",
        esc(title)
    );
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
    .into_bytes()
}

fn back(token: &str) -> String {
    format!(
        "<p><a href=\"/?t={}\">Back to the overview</a></p>",
        esc(token)
    )
}

fn budget(ws: &Workspace) -> usize {
    let paid = ws.replay().ok().is_some_and(|r| {
        r.state
            .get(col::PROFILE, "plan_tier")
            .and_then(Value::as_str)
            == Some("paid")
    });
    if paid { PAID_BUDGET } else { FREE_BUDGET }
}

fn today() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64 / 86_400)
}

fn status_body(ws: &Workspace, t: &str) -> String {
    let Ok(replay) = ws.replay() else {
        return "<h1>lucubra</h1><p>The workspace could not be read.</p>".into();
    };
    let state = replay.state;
    let count = |c: &str| state.collection(c).count();
    let fresh = build::build(
        &state,
        Options {
            today: today(),
            budget: budget(ws),
        },
    );
    let pack_state = match build::last_fingerprint(&state) {
        None => "No pack built yet.",
        Some(f) if f == fresh.fingerprint() => {
            "The pack in packs/ is up to date. A copy uploaded after the last build is current."
        }
        Some(_) => {
            "Your study data changed since the last pack. Rebuild it, then upload the new files."
        }
    };
    let te = esc(t);
    format!(
        "<h1>lucubra</h1><p>Workspace overview for {}.</p>\
<div class=\"card\"><h2>Workspace</h2><p>{} subjects, {} units, {} files, {} practice items, {} sessions.</p></div>\
<div class=\"card\"><h2>Study pack</h2><p>{pack_state}</p>\
<form method=\"post\" action=\"/pack?t={te}\"><button type=\"submit\">Rebuild the pack</button></form>\
<form method=\"post\" action=\"/open-packs?t={te}\"><p><button type=\"submit\">Open the packs folder</button></p></form></div>\
<div class=\"card\"><h2>Paste a handoff block</h2><form method=\"post\" action=\"/handoff?t={te}\">\
<label for=\"text\">Paste the whole block, from its first line to <code>end: lucubra-handoff</code>.</label>\
<textarea id=\"text\" name=\"text\" required></textarea><p><button type=\"submit\">Add to Lucubra</button></p></form></div>",
        iso_from_days(today()),
        count(col::SUBJECTS),
        count(col::UNITS),
        count(col::FILES),
        count(col::ITEMS),
        count(col::SESSIONS),
    )
}

fn handoff_body(ws: &Workspace, text: &str, t: &str) -> String {
    let outcomes = match ingest_text(ws, text, "companion") {
        Ok(o) => o,
        Err(e) => {
            return format!(
                "<h1>Handoff</h1><p>Lucubra could not save this block: {}</p>{}",
                esc(&e.to_string()),
                back(t)
            );
        }
    };
    let mut out = String::from("<h1>Handoff</h1>");
    if outcomes.is_empty() {
        out.push_str("<p>No handoff block was found. Copy the block starting at its <code>lucubra-handoff v1</code> line.</p>");
    }
    for o in &outcomes {
        out.push_str("<div class=\"card\">");
        match o {
            Outcome::Ingested(r) => {
                out.push_str(&format!(
                    "<p>Session {} added: {} steps, {} answers, {} card reviews, {} new questions.</p>",
                    esc(&r.session),
                    r.steps,
                    r.answers,
                    r.cards,
                    r.created.len()
                ));
                if !r.dropped.is_empty() {
                    out.push_str("<p>These questions were left out because their quote is not in your notes:</p><ul>");
                    for (prompt, why) in &r.dropped {
                        out.push_str(&format!("<li>{}: {}</li>", esc(prompt), esc(why)));
                    }
                    out.push_str("</ul>");
                }
            }
            Outcome::Duplicate(s) => out.push_str(&format!(
                "<p>Session {} was already added; nothing changed.</p>",
                esc(s)
            )),
            Outcome::Failed(why) => out.push_str(&format!("<p>Not added: {}</p>", esc(why))),
        }
        out.push_str("</div>");
    }
    out.push_str(&back(t));
    out
}

fn pack_body(ws: &Workspace, t: &str) -> String {
    let result = ws.replay().and_then(|r| {
        let pack = build::build(
            &r.state,
            Options {
                today: today(),
                budget: budget(ws),
            },
        );
        build::write(ws, &pack).map(|_| pack)
    });
    match result {
        Ok(pack) => format!(
            "<h1>Study pack</h1><p>Rebuilt {} files (about {} tokens) in packs/. Upload them to your project, Gem or skill, or let your Drive connector read the folder.</p><ul>{}</ul>{}",
            pack.files.len(),
            pack.tokens(),
            pack.files
                .keys()
                .map(|f| format!("<li>{}</li>", esc(f)))
                .collect::<String>(),
            back(t)
        ),
        Err(e) => format!(
            "<h1>Study pack</h1><p>The pack could not be built: {}</p>{}",
            esc(&e.to_string()),
            back(t)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_url_decoding_and_escaping() {
        assert_eq!(url_decode("a+b%2Cc%3A%0A%zz%"), "a b,c:\n%zz%");
        assert_eq!(
            parse_form(b"t=abc&text=x%3Dy"),
            vec![("t".into(), "abc".into()), ("text".into(), "x=y".into())]
        );
        assert_eq!(esc("<a href='x'>&"), "&lt;a href=&#39;x&#39;&gt;&amp;");
        assert!(constant_eq(b"abc", b"abc"));
        assert!(!constant_eq(b"abc", b"abd"));
        assert!(!constant_eq(b"abc", b"ab"));
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
