// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M7 companion page gates (S41, SPEC section 21): token, Host and Origin
//! checks before any work, size limits, paste ingest and pack rebuild.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;

use lucubra_companion::Companion;
use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_pack::handoff::EXAMPLE;

fn send(addr: SocketAddr, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(raw).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn get(addr: SocketAddr, path: &str, host: &str) -> String {
    send(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
    )
}

fn post(addr: SocketAddr, path: &str, host: &str, extra: &str, body: &str) -> String {
    send(
        addr,
        format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n{extra}\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}

fn form_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn status(resp: &str) -> &str {
    resp.split_whitespace().nth(1).unwrap_or_default()
}

#[test]
fn companion_rejects_without_token_host_or_origin_and_works_with_them() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    Workspace::init(root.path(), data.path()).unwrap();
    let server = Companion::bind().unwrap();
    let addr = server.addr();
    assert!(addr.ip().is_loopback());
    let token = server.token().to_owned();
    assert!(server.url().contains(&token));
    let (r, d) = (root.path().to_owned(), data.path().to_owned());
    let handle = thread::spawn(move || {
        let ws = Workspace::open(&r, &d).unwrap();
        server.serve_n(&ws, 11).unwrap();
    });
    let host = addr.to_string();
    let block = format!("text={}", form_encode(EXAMPLE));
    let ok = |p: &str| format!("{p}?t={token}");

    // Rejected before any work: no token, wrong token, foreign Host (DNS
    // rebinding), a Host name other than the bound address, foreign Origin.
    assert_eq!(status(&get(addr, "/", &host)), "403");
    assert_eq!(status(&get(addr, "/?t=wrong", &host)), "403");
    assert_eq!(status(&get(addr, &ok("/"), "evil.example:80")), "403");
    assert_eq!(status(&get(addr, &ok("/"), "localhost")), "403");
    let foreign = post(
        addr,
        &ok("/handoff"),
        &host,
        "Origin: http://evil.example\r\n",
        &block,
    );
    assert_eq!(status(&foreign), "403");
    assert_eq!(status(&post(addr, "/handoff", &host, "", &block)), "403");
    let big = send(
        addr,
        format!(
            "POST {} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 99999999\r\n\r\n",
            ok("/handoff")
        )
        .as_bytes(),
    );
    assert_eq!(status(&big), "413");

    // With the token.
    let page = get(addr, &ok("/"), &host);
    assert_eq!(status(&page), "200", "{page}");
    assert!(page.contains("No pack built yet."));
    assert!(page.contains("Content-Security-Policy"));
    let same_origin = format!("Origin: http://{host}\r\n");
    let added = post(addr, &ok("/handoff"), &host, &same_origin, &block);
    assert!(added.contains("Session 7b1e9c2a-55aa added"), "{added}");
    assert!(
        added.contains("left out"),
        "unknown source dropped and listed"
    );
    let again = post(addr, &ok("/handoff"), &host, "", &block);
    assert!(again.contains("already added"), "{again}");
    let pack = post(addr, &ok("/pack"), &host, "", "");
    assert!(pack.contains("Rebuilt"), "{pack}");
    handle.join().unwrap();

    let ws = Workspace::open(root.path(), data.path()).unwrap();
    let state = ws.replay().unwrap().state;
    assert_eq!(state.collection(col::HANDOFFS).count(), 1);
    assert!(root.path().join("packs/PACK.md").is_file());
}
