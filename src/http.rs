// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! Sending one record to mono-collector over HTTPS, through the router's curl.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const CURL: &str = "/usr/bin/curl";

#[derive(Debug, Clone, PartialEq)]
pub enum SendOutcome {
    /// The collector answered 204.
    Stored,
    /// The collector answered with another HTTP status.
    Rejected(u16),
    /// The name did not resolve or the connection was refused; curl's exit code.
    Unreachable(i32),
    /// The TLS handshake failed or the certificate is not trusted; curl's exit code.
    Tls(i32),
    /// curl exit 28.
    Timeout,
    /// curl ended with another exit code; -1 when it could not start.
    Failed(i32),
}

impl SendOutcome {
    /// The outcome's machine word, as the status file keeps it.
    pub fn word(&self) -> &'static str {
        match self {
            SendOutcome::Stored => "stored",
            SendOutcome::Rejected(_) => "rejected",
            SendOutcome::Unreachable(_) => "unreachable",
            SendOutcome::Tls(_) => "tls",
            SendOutcome::Timeout => "timeout",
            SendOutcome::Failed(_) => "failed",
        }
    }

    /// The HTTP status of a rejection or curl's exit code of any other failure.
    /// None for Stored, which is always 204.
    pub fn code(&self) -> Option<i64> {
        match self {
            SendOutcome::Rejected(status) => Some(*status as i64),
            SendOutcome::Unreachable(exit) | SendOutcome::Tls(exit) | SendOutcome::Failed(exit) => Some(*exit as i64),
            SendOutcome::Timeout => Some(28),
            SendOutcome::Stored => None,
        }
    }
}

pub trait Sender {
    fn send(&self, body: &str) -> SendOutcome;
}

/// Runs a program with its arguments and stdin, and gives back its exit code
/// and stdout; Err when it cannot start. The seam that lets tests run without curl.
pub trait Runner {
    fn run(&self, exe: &str, argv: &[String], stdin: &str) -> Result<(i32, String), String>;
}

/// ProcessRunner starts the program with only PATH in its environment. Its
/// stderr is the plugin's, so curl's own words reach the system log.
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, exe: &str, argv: &[String], stdin: &str) -> Result<(i32, String), String> {
        let mut child = Command::new(exe)
            .args(argv)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{exe}: {e}"))?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(stdin.as_bytes()).map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        Ok((output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

/// CurlSender posts the record with curl. The URL and the token go in a curl
/// config on stdin, never in argv, which `ps` shows to every account. The
/// body goes in a file only this account reads.
pub struct CurlSender {
    pub url: String,
    pub token: String,
    pub ca: String,
    /// Where the body file is written: the plugin's own run directory.
    pub dir: PathBuf,
    pub runner: Box<dyn Runner>,
}

impl CurlSender {
    fn body_path(&self) -> PathBuf {
        self.dir.join("record.json")
    }

    fn argv(&self) -> Vec<String> {
        let body = format!("@{}", self.body_path().display());
        [
            "-K", "-", "-s", "-m", "10", "--proto", "=https", "-o", "/dev/null", "-w", "%{http_code}",
            "--data-binary", &body,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// The curl config. settings::from_env has already refused any value with a
    /// quote, a backslash or a space, so each goes in a quoted string as it is.
    fn config(&self) -> String {
        let mut config = format!(
            "url = \"{}\"\nheader = \"X-Diag-Token: {}\"\nheader = \"Content-Type: application/json\"\n",
            self.url, self.token
        );
        if !self.ca.is_empty() {
            config.push_str(&format!("cacert = \"{}\"\n", self.ca));
        }
        config
    }
}

impl Sender for CurlSender {
    fn send(&self, body: &str) -> SendOutcome {
        if let Err(e) = write_private(&self.body_path(), body) {
            eprintln!("verso-plugin-diag: write {}: {e}", self.body_path().display());
            return SendOutcome::Failed(-1);
        }
        match self.runner.run(CURL, &self.argv(), &self.config()) {
            Ok((exit, stdout)) => outcome(exit, &stdout),
            Err(e) => {
                eprintln!("verso-plugin-diag: {e}");
                SendOutcome::Failed(-1)
            }
        }
    }
}

fn write_private(path: &PathBuf, text: &str) -> std::io::Result<()> {
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(text.as_bytes())
}

/// What curl's exit code and `%{http_code}` mean.
pub fn outcome(exit: i32, stdout: &str) -> SendOutcome {
    match exit {
        0 => match stdout.trim().parse::<u16>() {
            Ok(204) => SendOutcome::Stored,
            Ok(status) => SendOutcome::Rejected(status),
            Err(_) => SendOutcome::Failed(0),
        },
        6 | 7 => SendOutcome::Unreachable(exit),
        28 => SendOutcome::Timeout,
        35 | 51 | 53 | 54 | 58 | 59 | 60 | 66 | 77 | 80 | 83 | 90 | 91 => SendOutcome::Tls(exit),
        other => SendOutcome::Failed(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    /// FakeRunner answers with a fixed result and keeps what it was asked.
    struct FakeRunner {
        answer: Result<(i32, String), String>,
        seen: std::sync::Arc<Mutex<Vec<(String, Vec<String>, String)>>>,
    }

    impl Runner for FakeRunner {
        fn run(&self, exe: &str, argv: &[String], stdin: &str) -> Result<(i32, String), String> {
            self.seen.lock().unwrap().push((exe.into(), argv.to_vec(), stdin.into()));
            self.answer.clone()
        }
    }

    type Seen = std::sync::Arc<Mutex<Vec<(String, Vec<String>, String)>>>;

    fn sender(answer: Result<(i32, String), String>, ca: &str) -> (CurlSender, Seen, crate::sysfs::tree::Tree) {
        let t = crate::sysfs::tree::Tree::new();
        let seen: Seen = Default::default();
        let s = CurlSender {
            url: "https://diag.example.com/d".into(),
            token: "SuperSecretKey".into(),
            ca: ca.into(),
            dir: t.root.clone(),
            runner: Box::new(FakeRunner { answer, seen: seen.clone() }),
        };
        (s, seen, t)
    }

    #[test]
    fn each_answer_maps_to_its_outcome() {
        let cases = [
            (Ok((0, "204".to_string())), SendOutcome::Stored),
            (Ok((0, "401".to_string())), SendOutcome::Rejected(401)),
            (Ok((7, "000".to_string())), SendOutcome::Unreachable(7)),
            (Ok((6, "000".to_string())), SendOutcome::Unreachable(6)),
            (Ok((60, "000".to_string())), SendOutcome::Tls(60)),
            (Ok((35, "000".to_string())), SendOutcome::Tls(35)),
            (Ok((28, "000".to_string())), SendOutcome::Timeout),
            (Ok((2, "".to_string())), SendOutcome::Failed(2)),
            (Err("no curl".to_string()), SendOutcome::Failed(-1)),
        ];
        for (answer, want) in cases {
            let (s, _, _t) = sender(answer.clone(), "");
            assert_eq!(s.send("{}"), want, "{answer:?}");
        }
    }

    #[test]
    fn the_token_is_on_stdin_and_never_in_argv() {
        let (s, seen, _t) = sender(Ok((0, "204".into())), "");
        s.send("{}");
        let (exe, argv, stdin) = seen.lock().unwrap()[0].clone();
        assert_eq!(exe, "/usr/bin/curl");
        assert!(argv.iter().all(|a| !a.contains("SuperSecretKey") && !a.contains("diag.example.com")));
        assert!(stdin.contains("url = \"https://diag.example.com/d\"\n"));
        assert!(stdin.contains("header = \"X-Diag-Token: SuperSecretKey\"\n"));
        assert!(stdin.contains("header = \"Content-Type: application/json\"\n"));
        assert!(!stdin.contains("cacert"));
    }

    #[test]
    fn curl_is_held_to_https() {
        let (s, seen, _t) = sender(Ok((0, "204".into())), "");
        s.send("{}");
        let argv = seen.lock().unwrap()[0].1.clone();
        let at = argv.iter().position(|a| a == "--proto").unwrap();
        assert_eq!(argv[at + 1], "=https");
    }

    #[test]
    fn a_ca_goes_in_the_config() {
        let (s, seen, _t) = sender(Ok((0, "204".into())), "/etc/verso-plugin-diag/ca.pem");
        s.send("{}");
        assert!(seen.lock().unwrap()[0].2.contains("cacert = \"/etc/verso-plugin-diag/ca.pem\"\n"));
    }

    #[test]
    fn the_body_file_holds_the_record_for_this_account_alone() {
        let (s, seen, t) = sender(Ok((0, "204".into())), "");
        s.send(r#"{"deviceId":"e8:f6:d7:00:17:df"}"#);
        let path = t.root.join("record.json");
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"deviceId":"e8:f6:d7:00:17:df"}"#);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let argv = seen.lock().unwrap()[0].1.clone();
        assert!(argv.contains(&format!("@{}", path.display())));
    }

    #[test]
    fn a_second_send_replaces_the_body() {
        let (s, _, t) = sender(Ok((0, "204".into())), "");
        s.send("first, longer body");
        s.send("second");
        assert_eq!(fs::read_to_string(t.root.join("record.json")).unwrap(), "second");
    }
}
