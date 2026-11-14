// Operation journal, one JSON object per line. Callers must not put secrets in
// entries; backend errors never contain them.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Msg, OmniError, Result};

/// Above this size, the oldest half of the journal is dropped.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub level: LogLevel,
    pub action: String,
    pub status: String,
    pub detail: Option<String>,
}

pub fn append(path: &Path, entry: &LogEntry) -> Result<()> {
    append_with_limit(path, entry, MAX_LOG_BYTES)
}

fn append_with_limit(path: &Path, entry: &LogEntry, max_bytes: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(entry).map_err(|e| OmniError::Config(Msg::new("log.encode").detail(e)))?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    drop(file);
    if std::fs::metadata(path)?.len() > max_bytes {
        drop_oldest_half(path)?;
    }
    Ok(())
}

fn drop_oldest_half(path: &Path) -> Result<()> {
    let data = std::fs::read(path)?;
    let middle = data.len() / 2;
    // Keep whole lines only.
    let start = data[middle..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(data.len(), |i| middle + i + 1);
    let tmp = path.with_extension("log.tmp");
    std::fs::write(&tmp, &data[start..])?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The most recent entries, oldest first. Unreadable lines are skipped.
pub fn read_recent(path: &Path, limit: usize) -> Result<Vec<LogEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&data);
    let mut entries: Vec<LogEntry> = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(limit)
        .collect();
    entries.reverse();
    Ok(entries)
}

pub fn clear(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: i64) -> LogEntry {
        LogEntry {
            timestamp: n,
            level: LogLevel::Info,
            action: "Upload".into(),
            status: "OK".into(),
            detail: Some(format!("file {n}")),
        }
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("omniscp-log-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("nested").join("omniscp.log")
    }

    #[test]
    fn entries_survive_and_keep_their_order() {
        let path = scratch("order");
        assert!(read_recent(&path, 10).unwrap().is_empty());
        for n in 0..5 {
            append(&path, &entry(n)).unwrap();
        }
        let recent: Vec<i64> = read_recent(&path, 3).unwrap().iter().map(|e| e.timestamp).collect();
        assert_eq!(recent, [2, 3, 4]);
        clear(&path).unwrap();
        clear(&path).unwrap(); // idempotent
        assert!(read_recent(&path, 10).unwrap().is_empty());
    }

    #[test]
    fn damaged_lines_do_not_lose_the_rest() {
        let path = scratch("damaged");
        append(&path, &entry(1)).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{not json").unwrap();
        drop(file);
        append(&path, &entry(2)).unwrap();
        let recent: Vec<i64> = read_recent(&path, 10).unwrap().iter().map(|e| e.timestamp).collect();
        assert_eq!(recent, [1, 2]);
    }

    #[test]
    fn oversized_journal_drops_the_oldest_entries() {
        let path = scratch("rotate");
        for n in 0..1000 {
            append_with_limit(&path, &entry(n), 8_000).unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().len() <= 8_000);
        let recent = read_recent(&path, usize::MAX).unwrap();
        assert_eq!(recent.last().unwrap().timestamp, 999);
        assert!(recent.len() > 10);
        assert!(recent.first().unwrap().timestamp > 0);
        // Still contiguous: rotation only cuts between whole lines.
        assert!(recent.windows(2).all(|w| w[1].timestamp == w[0].timestamp + 1));
    }
}
