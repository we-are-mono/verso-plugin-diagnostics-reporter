// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! The push loop's state, written for the page to read. It never holds the token.

use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const FILE: &str = "status.json";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Status {
    /// None when the device ID could not be resolved.
    pub device_id: Option<String>,
    /// The collector's URL.
    pub collector: String,
    /// Why the push loop stopped.
    pub error: Option<String>,
    /// When the push loop started, in epoch seconds.
    pub started: u64,
    pub last_attempt: Option<u64>,
    /// The last attempt's outcome word (http::SendOutcome::word).
    pub last_outcome: Option<String>,
    /// The last attempt's HTTP status or curl exit code, where it has one.
    pub last_code: Option<i64>,
    pub last_stored: Option<u64>,
    pub sent: u64,
    pub failed: u64,
}

impl Status {
    fn to_json(&self) -> Value {
        json!({
            "device_id": self.device_id,
            "collector": self.collector,
            "error": self.error,
            "started": self.started,
            "last_attempt": self.last_attempt,
            "last_outcome": self.last_outcome,
            "last_code": self.last_code,
            "last_stored": self.last_stored,
            "sent": self.sent,
            "failed": self.failed,
        })
    }

    fn from_json(v: &Value) -> Option<Status> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        let number = |k: &str| v.get(k).and_then(Value::as_u64);
        Some(Status {
            device_id: text("device_id"),
            collector: text("collector")?,
            error: text("error"),
            started: number("started")?,
            last_attempt: number("last_attempt"),
            last_outcome: text("last_outcome"),
            last_code: v.get("last_code").and_then(Value::as_i64),
            last_stored: number("last_stored"),
            sent: number("sent")?,
            failed: number("failed")?,
        })
    }
}

/// Writes the status under a temporary name in `dir`, then renames it into
/// place, so a reader never sees half a file. Mode 0640.
pub fn write(dir: &Path, status: &Status) -> std::io::Result<()> {
    let tmp = dir.join(".status.json.tmp");
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o640).open(&tmp)?;
    file.write_all(status.to_json().to_string().as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, dir.join(FILE))
}

/// The status in `dir`. None when the file is missing or does not parse.
pub fn read(dir: &Path) -> Option<Status> {
    let text = fs::read_to_string(dir.join(FILE)).ok()?;
    Status::from_json(&serde_json::from_str(&text).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::tree::Tree;
    use std::os::unix::fs::PermissionsExt;

    fn full() -> Status {
        Status {
            device_id: Some("e8:f6:d7:00:17:df".into()),
            collector: "https://diag.example.com/d".into(),
            error: None,
            started: 1_793_829_000,
            last_attempt: Some(1_793_829_317),
            last_outcome: Some("rejected".into()),
            last_code: Some(401),
            last_stored: Some(1_793_828_417),
            sent: 3,
            failed: 1,
        }
    }

    #[test]
    fn a_written_status_reads_back_the_same() {
        let t = Tree::new();
        write(&t.root, &full()).unwrap();
        assert_eq!(read(&t.root), Some(full()));
        let empty = Status { collector: "https://x/d".into(), ..Default::default() };
        write(&t.root, &empty).unwrap();
        assert_eq!(read(&t.root), Some(empty));
    }

    #[test]
    fn the_file_is_not_world_readable() {
        let t = Tree::new();
        write(&t.root, &full()).unwrap();
        let mode = fs::metadata(t.root.join(FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_none() {
        let t = Tree::new();
        assert_eq!(read(&t.root), None);
        t.file("/status.json", "{\"collector\":");
        assert_eq!(read(&t.root), None);
    }
}
