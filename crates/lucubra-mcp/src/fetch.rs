// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Domain-restricted download for `lucubra_ingest(action="fetch")` (SPEC round
//! 4, A5, D40, S52).
//!
//! The engine downloads only when the user asks, only from a host listed in
//! `SOURCES.md` (or a subdomain of one), and only over HTTPS. Plain HTTP is
//! allowed for loopback hosts so tests run without the internet. Redirects are
//! followed by hand so every hop is checked against the list. The request
//! carries no cookies, no referrer and no workspace data. The caller records
//! the host and the saved file name, never the path or the query.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Redirect hops followed at most.
pub const MAX_REDIRECTS: usize = 5;
/// Timeout for the whole request.
pub const TIMEOUT_SECS: u64 = 60;

/// Why a fetch was refused or failed. Messages name the host at most.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The URL is malformed or uses an unsupported form.
    BadUrl(String),
    /// The host is not in `SOURCES.md`.
    Untrusted(String),
    /// The download failed or broke a limit.
    Failed(String),
}

/// A parsed URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `http` or `https`.
    pub scheme: String,
    /// Lower-case host name or IPv4 address.
    pub host: String,
    /// Explicit port, if any.
    pub port: Option<u16>,
    /// The path with any query, starting with `/`.
    pub path: String,
}

impl Target {
    fn url(&self) -> String {
        match self.port {
            Some(p) => format!("{}://{}:{p}{}", self.scheme, self.host, self.path),
            None => format!("{}://{}{}", self.scheme, self.host, self.path),
        }
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1"
}

/// Parses an absolute URL. Refuses user info, IPv6 literals, non-HTTP schemes
/// and plain HTTP to anything but loopback.
pub fn parse(url: &str) -> Result<Target, FetchError> {
    let bad = |m: &str| FetchError::BadUrl(m.to_owned());
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| bad("url must start with https://"))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Err(bad("url must start with https://"));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') {
        return Err(bad("url must not contain a user name or password"));
    }
    if authority.starts_with('[') {
        return Err(bad("IPv6 address URLs are not supported"));
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (
            h,
            Some(
                p.parse::<u16>()
                    .map_err(|_| bad("url has an invalid port"))?,
            ),
        ),
        None => (authority, None),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(bad("url has no valid host"));
    }
    if scheme == "http" && !is_loopback(&host) {
        return Err(bad("url must use https://"));
    }
    let path = path.split('#').next().unwrap_or("");
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    Ok(Target {
        scheme,
        host,
        port,
        path,
    })
}

/// Whether `host` equals a listed domain or is a subdomain of one.
pub fn allowed(host: &str, domains: &[String]) -> bool {
    domains
        .iter()
        .any(|d| host == d || host.ends_with(&format!(".{d}")))
}

/// A safe file name from the last path segment, without the query.
pub fn file_name(path: &str, content_type: Option<&str>) -> String {
    let last = path.split(['?', '#']).next().unwrap_or("");
    let last = last.rsplit('/').next().unwrap_or("");
    let mut name: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    name = name.trim_matches(['.', '-']).to_owned();
    name.truncate(100);
    if name.is_empty() {
        name = "download".into();
    }
    if !name.contains('.') {
        let mime = content_type.map(|t| t.split(';').next().unwrap_or("").trim());
        name.push_str(match mime {
            Some("application/pdf") => ".pdf",
            Some("text/html") => ".html",
            Some("text/plain") => ".txt",
            Some("text/markdown") => ".md",
            _ => "",
        });
    }
    name
}

fn unique(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), format!(".{e}")),
        None => (name.to_owned(), String::new()),
    };
    (1..)
        .map(|i| dir.join(format!("{stem}-{i}{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// What a successful fetch saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Host the file came from, after redirects.
    pub host: String,
    /// Saved file name inside `inbox/`.
    pub file: String,
    /// Bytes written.
    pub bytes: u64,
}

fn failed(m: impl Into<String>) -> FetchError {
    FetchError::Failed(m.into())
}

/// Downloads `url` into `inbox` when its host, and every redirect hop, is in
/// `domains`. The body is streamed to a `.part` file that is renamed only when
/// complete, so a failed download leaves nothing behind.
pub fn fetch(
    url: &str,
    domains: &[String],
    inbox: &Path,
    max_bytes: u64,
) -> Result<Saved, FetchError> {
    let mut target = parse(url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build()
        .into();
    for _ in 0..=MAX_REDIRECTS {
        if !allowed(&target.host, domains) {
            return Err(FetchError::Untrusted(target.host));
        }
        let response = agent
            .get(&target.url())
            .call()
            .map_err(|_| failed(format!("{} could not be reached", target.host)))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| failed("a redirect had no location"))?;
            target = if location.starts_with('/') {
                Target {
                    path: location.to_owned(),
                    ..target
                }
            } else {
                parse(location)?
            };
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(failed(format!(
                "{} answered with status {status}",
                target.host
            )));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        std::fs::create_dir_all(inbox).map_err(|_| failed("inbox/ could not be created"))?;
        let dest = unique(inbox, &file_name(&target.path, content_type.as_deref()));
        let mut part = dest.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        let written = (|| {
            let mut out = std::fs::File::create(&part)
                .map_err(|_| failed("the download could not be saved"))?;
            let mut body = response.into_body().into_reader().take(max_bytes + 1);
            let n = std::io::copy(&mut body, &mut out)
                .map_err(|_| failed("the download was interrupted"))?;
            if n > max_bytes {
                return Err(failed(format!(
                    "the file is larger than the {} MB input limit",
                    max_bytes >> 20
                )));
            }
            out.flush()
                .map_err(|_| failed("the download could not be saved"))?;
            Ok(n)
        })()
        .and_then(|n| {
            std::fs::rename(&part, &dest)
                .map(|()| n)
                .map_err(|_| failed("the download could not be saved"))
        });
        let written = written.inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })?;
        return Ok(Saved {
            host: target.host,
            file: dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            bytes: written,
        });
    }
    Err(failed(format!("more than {MAX_REDIRECTS} redirects")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_refuses_unsafe_forms() {
        assert!(parse("ftp://a.edu/x").is_err());
        assert!(
            parse("http://univ.edu/x").is_err(),
            "plain http off loopback"
        );
        assert!(parse("https://user:pw@univ.edu/x").is_err());
        assert!(parse("https://[::1]/x").is_err());
        assert!(parse("https:///x").is_err());
        assert!(parse("https://univ.edu:99999/x").is_err());
        let t = parse("https://Portal.Univ.EDU:443/papers/2024.pdf?token=1#p").unwrap();
        assert_eq!(t.host, "portal.univ.edu");
        assert_eq!(t.port, Some(443));
        assert_eq!(t.path, "/papers/2024.pdf?token=1");
        assert_eq!(parse("https://univ.edu").unwrap().path, "/");
        assert!(parse("http://127.0.0.1:8080/a").is_ok());
    }

    #[test]
    fn allow_list_matches_hosts_and_subdomains_only() {
        let d = vec!["univ.edu".to_owned()];
        assert!(allowed("univ.edu", &d));
        assert!(allowed("exams.univ.edu", &d));
        assert!(!allowed("evilunivedu", &d));
        assert!(!allowed("univ.edu.evil.com", &d));
        assert!(!allowed("notuniv.edu", &d));
    }

    #[test]
    fn file_names_drop_the_query_and_unsafe_characters() {
        assert_eq!(
            file_name("/a/2024 paper.pdf?sig=abc", None),
            "2024-paper.pdf"
        );
        assert_eq!(file_name("/a/../..", None), "download");
        assert_eq!(file_name("/q", Some("application/pdf; x")), "q.pdf");
    }
}
