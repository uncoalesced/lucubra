// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Calendar read (SPEC section 17): busy time from an iCal feed or from
//! host-supplied blocks, and storage of the secret iCal address in app data.
//! The address works like a password, so it never enters the workspace, the
//! event log or any message.

use std::fs;
use std::path::PathBuf;

use crate::planner::days_from_iso;
use crate::{Error, Result, Workspace};

const URL_FILE: &str = "calendar.url";

fn url_path(ws: &Workspace) -> Result<PathBuf> {
    Ok(ws.cache_dir()?.join(URL_FILE))
}

/// Stores (or with `None`, removes) the iCal address in app data.
pub fn store_url(ws: &Workspace, url: Option<&str>) -> Result<()> {
    let path = url_path(ws)?;
    match url {
        Some(url) => {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)
                    .map_err(|e| Error::io("creating calendar settings folder", e))?;
            }
            fs::write(&path, url.trim()).map_err(|e| Error::io("saving calendar address", e))
        }
        None => match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(Error::io("removing calendar address", e))
            }
            _ => Ok(()),
        },
    }
}

/// The stored iCal address, if any.
pub fn stored_url(ws: &Workspace) -> Option<String> {
    let raw = fs::read_to_string(url_path(ws).ok()?).ok()?;
    let url = raw.trim();
    (!url.is_empty()).then(|| url.to_owned())
}

/// Minutes since the epoch for `YYYYMMDDTHHMMSS[Z]` or RFC 3339
/// `YYYY-MM-DDTHH:MM[:SS][Z]`. Offsets other than `Z` are ignored.
///
/// shortcut: times are taken as written (UTC or floating), not converted to
/// the user's zone; add TZID handling when a tester's busy blocks shift.
pub fn parse_minutes(stamp: &str) -> Option<i64> {
    let compact: String = stamp
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .collect();
    let (date, time) = compact.split_once('T')?;
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let iso = format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..8]);
    let day = days_from_iso(&iso)?;
    let h: i64 = time[..2].parse().ok()?;
    let m: i64 = time[2..4].parse().ok()?;
    Some(day * 1440 + h * 60 + m)
}

/// Timed events in an iCal text as `(start, end)` minutes since the epoch.
/// All-day events are skipped: they mark days, not busy hours.
pub fn ics_intervals(ics: &str) -> Vec<(i64, i64)> {
    // Unfold continuation lines (RFC 5545 section 3.1).
    let unfolded = ics
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "");
    let mut out = Vec::new();
    let (mut start, mut end) = (None, None);
    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            (start, end) = (None, None);
        } else if line == "END:VEVENT" {
            if let (Some(s), Some(e)) = (start, end)
                && e > s
            {
                out.push((s, e));
            }
        } else if let Some((key, value)) = line.split_once(':') {
            if key.contains("VALUE=DATE") && !key.contains("DATE-TIME") {
                continue;
            }
            if key.starts_with("DTSTART") {
                start = parse_minutes(value);
            } else if key.starts_with("DTEND") {
                end = parse_minutes(value);
            }
        }
    }
    out
}

/// Busy minutes on each of `days` days starting at day `today`.
pub fn busy_per_day(intervals: &[(i64, i64)], today: i64, days: usize) -> Vec<u32> {
    (0..days as i64)
        .map(|d| {
            let (lo, hi) = ((today + d) * 1440, (today + d + 1) * 1440);
            intervals
                .iter()
                .map(|(s, e)| ((*e).min(hi) - (*s).max(lo)).max(0))
                .sum::<i64>()
                .min(1440) as u32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261124T090000Z\r\nDTEND:20261124T1\r\n 03000Z\r\nSUMMARY:Lab\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261125\r\nDTEND;VALUE=DATE:20261126\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Kolkata:20261125T233000\r\nDTEND;TZID=Asia/Kolkata:20261126T003000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn calendar_parses_timed_events_and_splits_midnight() {
        let iv = ics_intervals(FEED);
        assert_eq!(iv.len(), 2, "all-day event skipped, folded line joined");
        let today = days_from_iso("2026-11-24").unwrap();
        assert_eq!(busy_per_day(&iv, today, 3), vec![90, 30, 30]);
        assert_eq!(
            parse_minutes("2026-11-24T09:15:00Z"),
            parse_minutes("20261124T091500Z")
        );
    }

    #[test]
    fn calendar_url_lives_in_app_data_not_the_workspace() {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = Workspace::init(root.path(), data.path()).unwrap();
        let secret = "https://calendar.example/private-abc123/basic.ics";
        store_url(&ws, Some(secret)).unwrap();
        assert_eq!(stored_url(&ws).as_deref(), Some(secret));
        let mut stack = vec![root.path().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for e in fs::read_dir(dir).unwrap().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    let bytes = fs::read(e.path()).unwrap();
                    assert!(!String::from_utf8_lossy(&bytes).contains("private-abc123"));
                }
            }
        }
        store_url(&ws, None).unwrap();
        assert!(stored_url(&ws).is_none());
    }
}
