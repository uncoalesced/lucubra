// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Per-device append-only event logs (SPEC section 6.1, ADR 0008).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::event::{Event, RECORD_VERSION, format_ts, parse_line};
use crate::hlc::{Clock, Hlc};

/// Largest lead over physical time accepted from another device's clock.
pub const MAX_FUTURE_MS: u64 = 24 * 60 * 60 * 1000;

/// Bytes read from the end of a log to find its last record.
const TAIL_WINDOW: u64 = 64 * 1024;

/// A line in a log that is not a valid record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLine {
    /// Log file the line came from.
    pub file: PathBuf,
    /// One-based line number.
    pub line: usize,
    /// Whether this was the final line without a newline (a torn write).
    pub torn: bool,
}

/// Events and bad lines read from one or more logs.
#[derive(Debug, Default, Clone)]
pub struct LogRead {
    /// Valid records, in file order.
    pub events: Vec<Event>,
    /// Lines that were skipped.
    pub bad_lines: Vec<BadLine>,
}

/// Path of the log for `dev` inside an events directory.
pub fn log_path(events_dir: &Path, dev: &str) -> PathBuf {
    events_dir.join(format!("{dev}.jsonl"))
}

/// Appends one event to the device log and syncs it to disk.
///
/// `observed` is the greatest clock value seen in any log; the new event
/// orders after it and after the last event in this log.
pub fn append_event(
    events_dir: &Path,
    dev: &str,
    kind: &str,
    data: Value,
    observed: Option<&Hlc>,
    clock: &dyn Clock,
) -> Result<Event> {
    fs::create_dir_all(events_dir)
        .map_err(|e| Error::io(format!("create {}", events_dir.display()), e))?;
    let path = log_path(events_dir, dev);
    let mut file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|e| Error::io(format!("open {}", path.display()), e))?;
    file.lock()
        .map_err(|e| Error::io(format!("lock {}", path.display()), e))?;
    let result = append_locked(&mut file, &path, dev, kind, data, observed, clock);
    let unlock = file.unlock();
    let event = result?;
    unlock.map_err(|e| Error::io(format!("unlock {}", path.display()), e))?;
    Ok(event)
}

fn append_locked(
    file: &mut File,
    path: &Path,
    dev: &str,
    kind: &str,
    data: Value,
    observed: Option<&Hlc>,
    clock: &dyn Clock,
) -> Result<Event> {
    let ctx = |action: &str| format!("{action} {}", path.display());
    let (last, ends_with_newline) = read_tail(file).map_err(|e| Error::io(ctx("read"), e))?;
    let last_hlc = last.as_ref().and_then(|e| Hlc::parse(&e.hlc));
    let now = clock.now_ms();
    // A synced clock value far in the future (a bad clock or a crafted event)
    // must not drag this device's clock to the end of the encodable range.
    let observed = observed.filter(|h| h.ms <= now.saturating_add(MAX_FUTURE_MS));
    let base = Hlc::max_of(last_hlc, observed.cloned());
    let hlc = Hlc::tick(base.as_ref(), now, dev);
    if Hlc::parse(&hlc.to_string()).as_ref() != Some(&hlc) {
        return Err(Error::Invalid(format!(
            "clock value {hlc} for {} is outside the encodable range",
            path.display()
        )));
    }
    let event = Event {
        v: RECORD_VERSION,
        id: ulid::Ulid::new().to_string(),
        dev: dev.to_owned(),
        seq: last.map_or(1, |e| e.seq + 1),
        hlc: hlc.to_string(),
        ts: format_ts(now),
        kind: kind.to_owned(),
        data,
    };
    let mut line = String::new();
    if !ends_with_newline {
        // Keep a torn fragment on its own line; never truncate the log.
        line.push('\n');
    }
    line.push_str(&event.to_line());
    line.push('\n');
    file.write_all(line.as_bytes())
        .map_err(|e| Error::io(ctx("append"), e))?;
    file.sync_all().map_err(|e| Error::io(ctx("sync"), e))?;
    Ok(event)
}

/// Returns the last valid record in the file and whether the file is empty or
/// ends with a newline.
fn read_tail(file: &mut File) -> std::io::Result<(Option<Event>, bool)> {
    let len = file.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok((None, true));
    }
    let start = len.saturating_sub(TAIL_WINDOW);
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.read_to_end(&mut bytes)?;
    let ends_with_newline = bytes.last() == Some(&b'\n');
    if let Some(event) = last_record(&bytes, start > 0) {
        return Ok((Some(event), ends_with_newline));
    }
    if start == 0 {
        return Ok((None, ends_with_newline));
    }
    // No complete record in the window: scan the whole file.
    bytes.clear();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut bytes)?;
    Ok((last_record(&bytes, false), ends_with_newline))
}

fn last_record(bytes: &[u8], skip_first_partial: bool) -> Option<Event> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if skip_first_partial && !lines.is_empty() {
        lines.remove(0);
    }
    lines
        .iter()
        .rev()
        .find_map(|line| parse_line(line.trim_end_matches('\r')))
}

/// Reads one log file. Lines that do not parse are reported, not fatal.
pub fn read_log(path: &Path) -> Result<LogRead> {
    let mut file =
        File::open(path).map_err(|e| Error::io(format!("open {}", path.display()), e))?;
    file.lock_shared()
        .map_err(|e| Error::io(format!("lock {}", path.display()), e))?;
    let mut bytes = Vec::new();
    let read = file.read_to_end(&mut bytes);
    let _ = file.unlock();
    read.map_err(|e| Error::io(format!("read {}", path.display()), e))?;

    let text = String::from_utf8_lossy(&bytes);
    let ends_with_newline = bytes.is_empty() || bytes.last() == Some(&b'\n');
    let pieces: Vec<&str> = text.split('\n').collect();
    let count = pieces.len();
    let mut out = LogRead::default();
    for (i, piece) in pieces.into_iter().enumerate() {
        let line = piece.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(event) => out.events.push(event),
            None => out.bad_lines.push(BadLine {
                file: path.to_path_buf(),
                line: i + 1,
                torn: i + 1 == count && !ends_with_newline,
            }),
        }
    }
    Ok(out)
}

/// Reads every `*.jsonl` file in the events directory, including sync-conflict
/// copies of a log. Files are read in name order; replay sorts and de-duplicates.
pub fn read_all_logs(events_dir: &Path) -> Result<LogRead> {
    let mut out = LogRead::default();
    let entries = match fs::read_dir(events_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::io(format!("list {}", events_dir.display()), e)),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    paths.sort();
    for path in paths {
        let read = read_log(&path)?;
        out.events.extend(read.events);
        out.bad_lines.extend(read.bad_lines);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::FixedClock;
    use serde_json::json;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn append(dir: &Path, data: Value) -> Event {
        let clock = FixedClock(1_762_872_000_000);
        append_event(dir, "d-a", "setting.changed", data, None, &clock).unwrap()
    }

    #[test]
    fn appends_sequence_and_clock() {
        let dir = temp();
        let a = append(dir.path(), json!({}));
        let b = append(dir.path(), json!({}));
        assert_eq!((a.seq, b.seq), (1, 2));
        assert!(b.order_key().unwrap() > a.order_key().unwrap());
        let read = read_log(&log_path(dir.path(), "d-a")).unwrap();
        assert_eq!(read.events, vec![a, b]);
        assert!(read.bad_lines.is_empty());
    }

    #[test]
    fn new_events_order_after_observed_clock() {
        let dir = temp();
        let observed = Hlc::parse("1762872005000-0005-d-z").unwrap();
        let e = append_event(
            dir.path(),
            "d-a",
            "setting.changed",
            json!({}),
            Some(&observed),
            &FixedClock(1_762_872_000_000),
        )
        .unwrap();
        assert!(Hlc::parse(&e.hlc).unwrap() > observed);
    }

    #[test]
    fn far_future_observed_clock_is_ignored() {
        let dir = temp();
        let observed = Hlc::parse("9999999999999-9999-d-z").unwrap();
        let e = append_event(
            dir.path(),
            "d-a",
            "setting.changed",
            json!({}),
            Some(&observed),
            &FixedClock(1_762_872_000_000),
        )
        .unwrap();
        assert_eq!(Hlc::parse(&e.hlc).unwrap().ms, 1_762_872_000_000);
        assert_eq!(
            read_log(&log_path(dir.path(), "d-a")).unwrap().events.len(),
            1
        );
    }

    #[test]
    fn clock_overflow_is_an_error_not_a_lost_line() {
        let dir = temp();
        let path = log_path(dir.path(), "d-a");
        let last = r#"{"v":1,"id":"x","dev":"d-a","seq":1,"hlc":"9999999999999-9999-d-a","ts":"","type":"setting.changed","data":{}}"#;
        fs::write(&path, format!("{last}\n")).unwrap();
        let clock = FixedClock(9_999_999_999_999);
        let result = append_event(
            dir.path(),
            "d-a",
            "setting.changed",
            json!({}),
            None,
            &clock,
        );
        assert!(result.is_err());
        assert_eq!(read_log(&path).unwrap().events.len(), 1);
    }

    #[test]
    fn torn_last_line_is_skipped_and_next_append_is_intact() {
        let dir = temp();
        let first = append(dir.path(), json!({"k": 1}));
        let second = append(dir.path(), json!({"k": 2}));
        let path = log_path(dir.path(), "d-a");
        let bytes = fs::read(&path).unwrap();
        // Crash mid-write of the second record: cut it in half.
        let cut = bytes.len() - second.to_line().len() / 2;
        fs::write(&path, &bytes[..cut]).unwrap();

        let read = read_log(&path).unwrap();
        assert_eq!(read.events, vec![first.clone()]);
        assert_eq!(read.bad_lines.len(), 1);
        assert!(read.bad_lines[0].torn);

        let third = append(dir.path(), json!({"k": 3}));
        assert_eq!(third.seq, 2);
        let read = read_log(&path).unwrap();
        assert_eq!(read.events, vec![first, third]);
        assert_eq!(read.bad_lines.len(), 1);
        assert!(!read.bad_lines[0].torn);
        assert!(fs::read(&path).unwrap().ends_with(b"\n"));
    }

    #[test]
    fn missing_events_directory_reads_as_empty() {
        let dir = temp();
        let read = read_all_logs(&dir.path().join("none")).unwrap();
        assert!(read.events.is_empty());
    }
}
