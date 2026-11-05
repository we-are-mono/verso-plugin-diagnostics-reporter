// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! System › Mono feedback: what the push loop last did. The page only reads.

use crate::push::INTERVAL;
use crate::record::utc;
use crate::status::{self, Status};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use verso_plugin::{Envelope, Property, Request, Tone, Widget};

pub const TITLE: &str = "Mono feedback";

/// GET "/" is the page; any other path has nothing.
pub fn get(req: &Request, dir: &Path, now: SystemTime) -> Envelope {
    if req.path != "/" && !req.path.is_empty() {
        return Envelope::not_found();
    }
    page(status::read(dir).as_ref(), now)
}

fn page(status: Option<&Status>, now: SystemTime) -> Envelope {
    let mut children = Vec::new();
    match status {
        None => children.push(Widget::callout(
            Tone::Info,
            "The upload is not running",
            "It starts with the router. The system log has the reason under verso-plugin-diag.",
        )),
        Some(s) => {
            if let Some(error) = &s.error {
                children.push(Widget::callout(Tone::Danger, "The upload stopped", error));
            } else if stale(s, now) {
                children.push(Widget::callout(
                    Tone::Warning,
                    "No reading was sent in the last 30 minutes",
                    "The system log has the reason under verso-plugin-diag.",
                ));
            }
            children.push(Widget::properties(facts(s)));
        }
    }
    let mut envelope = Envelope::page(
        TITLE,
        Widget::section(
            "Status",
            "Sends this router's temperatures, fan speed and power draw to Mono every 15 minutes.",
            children,
        ),
    );
    envelope.width = "narrow".into();
    envelope
}

/// A status whose last attempt, or start with no attempt, is older than two
/// intervals: the loop is no longer running.
fn stale(s: &Status, now: SystemTime) -> bool {
    let now = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let last = s.last_attempt.unwrap_or(s.started);
    now > last + 2 * INTERVAL.as_secs()
}

fn data(label: &str, value: &str, mono: bool) -> Property {
    Property {
        label: label.into(),
        value: value.into(),
        mono,
        verbatim: !mono,
        copy: mono,
        ..Default::default()
    }
}

fn when(epoch: Option<u64>) -> Option<String> {
    epoch.map(|e| utc(UNIX_EPOCH + std::time::Duration::from_secs(e)).replace('T', " ").replace('Z', " UTC"))
}

fn facts(s: &Status) -> Vec<Property> {
    let mut rows = vec![
        data("Device ID", s.device_id.as_deref().unwrap_or("—"), true),
        data("Collector", if s.collector.is_empty() { "—" } else { &s.collector }, true),
    ];
    let mut attempt = Property { label: "Last attempt".into(), ..Default::default() };
    match when(s.last_attempt) {
        Some(at) => {
            attempt.value = at;
            attempt.verbatim = true;
        }
        None => attempt.value = "Not yet".into(),
    }
    rows.push(attempt);
    if let Some(word) = &s.last_outcome {
        rows.push(result(word, s.last_code));
    }
    let mut stored = Property { label: "Last stored".into(), ..Default::default() };
    match when(s.last_stored) {
        Some(at) => {
            stored.value = at;
            stored.verbatim = true;
        }
        None => stored.value = "Never".into(),
    }
    rows.push(stored);
    rows.push(data("Sent", &s.sent.to_string(), false));
    rows.push(data("Not sent", &s.failed.to_string(), false));
    rows
}

/// The last result in words, with the HTTP status or curl's exit code as its note.
fn result(word: &str, code: Option<i64>) -> Property {
    let (value, tone) = match word {
        "stored" => ("Stored", Tone::Success),
        "rejected" => ("Refused by the collector", Tone::Danger),
        "unreachable" => ("Collector unreachable", Tone::Danger),
        "tls" => ("Collector certificate not trusted", Tone::Danger),
        "timeout" => ("Timed out", Tone::Danger),
        _ => ("Failed", Tone::Danger),
    };
    let row = Property { label: "Last result".into(), value: value.into(), ..Default::default() }.marked(tone);
    match (word, code) {
        ("rejected", Some(c)) => row.noted(&format!("HTTP {c}")),
        ("failed", Some(c)) if c >= 0 => row.noted(&format!("curl exit {c}")),
        _ => row,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::tree::Tree;
    use std::time::Duration;
    use verso_plugin::{Form, Plugins, Snapshot, Ubus};

    const NOW: u64 = 1_793_829_317;

    fn request(path: &str) -> Request {
        Request {
            path: path.into(),
            query: Form::parse(""),
            snapshot: Snapshot::from_value(serde_json::json!({})),
            ubus: Ubus::from_value(serde_json::json!({})),
            plugins: Plugins::new(&["diag"]),
        }
    }

    fn render(status: Option<Status>) -> serde_json::Value {
        let t = Tree::new();
        if let Some(s) = status {
            status::write(&t.root, &s).unwrap();
        }
        let envelope = get(&request("/"), &t.root, UNIX_EPOCH + Duration::from_secs(NOW));
        serde_json::to_value(&envelope).unwrap()
    }

    fn running() -> Status {
        Status {
            device_id: Some("e8:f6:d7:00:17:df".into()),
            collector: "https://diag.example.com/d".into(),
            started: NOW - 1000,
            last_attempt: Some(NOW - 100),
            last_outcome: Some("stored".into()),
            last_stored: Some(NOW - 100),
            sent: 2,
            ..Default::default()
        }
    }

    fn section(v: &serde_json::Value) -> &Vec<serde_json::Value> {
        v["widget"]["children"].as_array().unwrap()
    }

    fn rows(v: &serde_json::Value) -> Vec<(String, String)> {
        let props = section(v).iter().find(|w| w["type"] == "properties").unwrap();
        props["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["label"].as_str().unwrap().into(), i["value"].as_str().unwrap().into()))
            .collect()
    }

    #[test]
    fn no_status_says_the_upload_is_not_running() {
        let v = render(None);
        assert_eq!(v["title"], "Mono feedback");
        let only = &section(&v)[..];
        assert_eq!(only.len(), 1);
        assert_eq!(only[0]["type"], "callout");
        assert_eq!(only[0]["title"], "The upload is not running");
    }

    #[test]
    fn a_running_upload_lists_its_facts() {
        let v = render(Some(running()));
        assert!(section(&v).iter().all(|w| w["type"] != "callout"));
        let rows = rows(&v);
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Device ID", "Collector", "Last attempt", "Last result", "Last stored", "Sent", "Not sent"]);
        assert_eq!(rows[0].1, "e8:f6:d7:00:17:df");
        assert_eq!(rows[2].1, "2026-11-04 21:53:37 UTC");
        assert_eq!(rows[3].1, "Stored");
        assert_eq!(rows[5].1, "2");
    }

    #[test]
    fn machine_values_are_mono_and_counts_verbatim() {
        let v = render(Some(running()));
        let props = section(&v).iter().find(|w| w["type"] == "properties").unwrap();
        let items = props["items"].as_array().unwrap();
        assert_eq!(items[0]["mono"], true);
        assert_eq!(items[1]["mono"], true);
        assert_eq!(items[5]["verbatim"], true);
    }

    #[test]
    fn an_error_is_said_in_a_danger_callout() {
        let mut s = running();
        s.error = Some("cannot read /sys/class/net/eth0/address".into());
        let v = render(Some(s));
        let callout = &section(&v)[0];
        assert_eq!(callout["variant"], "danger");
        assert_eq!(callout["body"], "cannot read /sys/class/net/eth0/address");
    }

    #[test]
    fn a_status_older_than_two_intervals_warns() {
        let mut s = running();
        s.last_attempt = Some(NOW - 1801);
        let v = render(Some(s));
        assert_eq!(section(&v)[0]["variant"], "warning");
    }

    #[test]
    fn a_refusal_carries_its_http_status() {
        let mut s = running();
        s.last_outcome = Some("rejected".into());
        s.last_code = Some(401);
        let v = render(Some(s));
        let props = section(&v).iter().find(|w| w["type"] == "properties").unwrap();
        let result = &props["items"][3];
        assert_eq!(result["value"], "Refused by the collector");
        assert_eq!(result["help"], "HTTP 401");
        assert_eq!(result["dot"], "danger");
    }

    #[test]
    fn no_attempt_yet_reads_not_yet() {
        let mut s = running();
        s.last_attempt = None;
        s.last_outcome = None;
        s.last_stored = None;
        let rows = rows(&render(Some(s)));
        assert!(rows.contains(&("Last attempt".into(), "Not yet".into())));
        assert!(rows.contains(&("Last stored".into(), "Never".into())));
        assert!(!rows.iter().any(|(l, _)| l == "Last result"));
    }

    #[test]
    fn another_path_is_not_found() {
        let t = Tree::new();
        let envelope = get(&request("/other"), &t.root, UNIX_EPOCH);
        assert_eq!(serde_json::to_value(&envelope).unwrap()["not_found"], true);
    }
}
