// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! The push loop: read the sensors and send one record every 15 minutes.

use crate::device_id::{self, DeviceId};
use crate::http::{CurlSender, ProcessRunner, SendOutcome, Sender};
use crate::record::{sample, to_json};
use crate::sensors::GATEWAY_DK;
use crate::settings::{self, PushSettings};
use crate::status::{self, Status};
use crate::sysfs::{DirSysFs, SysFs};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often a record is sent. Fixed: the collection runs for a short time and
/// nothing on the router configures it.
pub const INTERVAL: Duration = Duration::from_secs(900);

/// Clock gives the wall time for the records and a monotonic time for the
/// schedule, so a clock set by NTP after boot neither bunches nor skips sends.
pub trait Clock {
    fn wall(&self) -> SystemTime;
    /// Time since an arbitrary fixed start; never goes back.
    fn monotonic(&self) -> Duration;
    fn sleep(&self, d: Duration);
}

pub struct RealClock {
    start: Instant,
}

impl Clock for RealClock {
    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
    fn monotonic(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d)
    }
}

fn epoch(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One cycle: sample, send, and the status that follows `prev`.
pub fn tick(fs: &dyn SysFs, sender: &dyn Sender, clock: &dyn Clock, id: &DeviceId, prev: &Status) -> Status {
    let now = clock.wall();
    let record = sample(fs, GATEWAY_DK, id, now);
    let outcome = sender.send(&to_json(&record));
    let mut next = prev.clone();
    next.last_attempt = Some(epoch(now));
    next.last_outcome = Some(outcome.word().into());
    next.last_code = outcome.code();
    if outcome == SendOutcome::Stored {
        next.sent += 1;
        next.last_stored = Some(epoch(now));
    } else {
        next.failed += 1;
        eprintln!("verso-plugin-diag: sensor upload failed at {}: {outcome:?}", record.timestamp);
    }
    next
}

/// Sends a record at once and then every INTERVAL, `limit` times or for ever.
/// The status goes to `dir` before the first send and after each one. A device
/// ID that cannot be resolved is written to the status and ends the loop with 1.
pub fn run(
    settings: &PushSettings,
    fs: &dyn SysFs,
    sender: &dyn Sender,
    clock: &dyn Clock,
    dir: &Path,
    limit: Option<u64>,
) -> i32 {
    let mut status = Status { collector: settings.url.clone(), started: epoch(clock.wall()), ..Default::default() };
    let id = match device_id::resolve(fs, &settings.device_override) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("verso-plugin-diag: {e}");
            status.error = Some(e.to_string());
            save(dir, &status);
            return 1;
        }
    };
    status.device_id = Some(id.as_str().into());
    save(dir, &status);
    println!("verso-plugin-diag: sending as {} to {} every {} s", id.as_str(), settings.url, INTERVAL.as_secs());

    let start = clock.monotonic();
    let mut slot: u32 = 0;
    let mut done = 0;
    loop {
        let due = INTERVAL * slot;
        let elapsed = clock.monotonic() - start;
        if due > elapsed {
            clock.sleep(due - elapsed);
        }
        status = tick(fs, sender, clock, &id, &status);
        save(dir, &status);
        done += 1;
        if limit.is_some_and(|l| done >= l) {
            return 0;
        }
        // The next slot, or the current one when a slot was missed.
        let elapsed = clock.monotonic() - start;
        slot = (slot + 1).max((elapsed.as_secs() / INTERVAL.as_secs()) as u32);
    }
}

fn save(dir: &Path, status: &Status) {
    if let Err(e) = status::write(dir, status) {
        eprintln!("verso-plugin-diag: write status in {}: {e}", dir.display());
    }
}

/// `verso-plugin-diag push`, as the init script starts it.
pub fn main(dir: &Path) -> i32 {
    let settings = match settings::from_env(&|name| std::env::var(name).ok()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("verso-plugin-diag: {e}");
            save(dir, &Status { error: Some(e.to_string()), started: epoch(SystemTime::now()), ..Default::default() });
            return 1;
        }
    };
    let sender = CurlSender {
        url: settings.url.clone(),
        token: settings.token.clone(),
        ca: settings.ca.clone(),
        dir: dir.to_path_buf(),
        runner: Box::new(ProcessRunner),
    };
    let clock = RealClock { start: Instant::now() };
    run(&settings, &DirSysFs::real(), &sender, &clock, dir, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::dk;
    use crate::sysfs::tree::Tree;
    use std::cell::{Cell, RefCell};

    /// FakeClock moves only when slept, or when a send takes time.
    struct FakeClock {
        wall: Cell<u64>,
        mono: Cell<Duration>,
    }

    impl FakeClock {
        fn new() -> FakeClock {
            FakeClock { wall: Cell::new(1_793_829_317), mono: Cell::new(Duration::ZERO) }
        }
        fn pass(&self, d: Duration) {
            self.mono.set(self.mono.get() + d);
            self.wall.set(self.wall.get() + d.as_secs());
        }
    }

    impl Clock for FakeClock {
        fn wall(&self) -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(self.wall.get())
        }
        fn monotonic(&self) -> Duration {
            self.mono.get()
        }
        fn sleep(&self, d: Duration) {
            self.pass(d)
        }
    }

    /// FakeSender answers in turn and notes when each send began.
    struct FakeSender<'a> {
        answers: RefCell<Vec<SendOutcome>>,
        clock: &'a FakeClock,
        took: Duration,
        sent_at: RefCell<Vec<Duration>>,
        bodies: RefCell<Vec<String>>,
    }

    impl<'a> FakeSender<'a> {
        fn new(clock: &'a FakeClock, answers: Vec<SendOutcome>, took: Duration) -> FakeSender<'a> {
            FakeSender {
                answers: RefCell::new(answers),
                clock,
                took,
                sent_at: RefCell::default(),
                bodies: RefCell::default(),
            }
        }
    }

    impl Sender for FakeSender<'_> {
        fn send(&self, body: &str) -> SendOutcome {
            self.sent_at.borrow_mut().push(self.clock.monotonic());
            self.bodies.borrow_mut().push(body.into());
            self.clock.pass(self.took);
            let mut answers = self.answers.borrow_mut();
            if answers.len() > 1 {
                answers.remove(0)
            } else {
                answers[0].clone()
            }
        }
    }

    fn settings() -> PushSettings {
        PushSettings {
            url: "https://diag.example.com/d".into(),
            token: "SuperSecretKey".into(),
            ca: String::new(),
            device_override: String::new(),
        }
    }

    #[test]
    fn a_stored_send_counts_as_sent() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::ZERO);
        let id = device_id::resolve(&t.fs(), "").unwrap();
        let s = tick(&t.fs(), &sender, &clock, &id, &Status::default());
        assert_eq!((s.sent, s.failed), (1, 0));
        assert_eq!(s.last_outcome.as_deref(), Some("stored"));
        assert_eq!(s.last_stored, Some(1_793_829_317));
        assert_eq!(s.last_attempt, Some(1_793_829_317));
    }

    #[test]
    fn a_rejected_send_counts_as_failed_with_its_status() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Rejected(401)], Duration::ZERO);
        let id = device_id::resolve(&t.fs(), "").unwrap();
        let s = tick(&t.fs(), &sender, &clock, &id, &Status::default());
        assert_eq!((s.sent, s.failed), (0, 1));
        assert_eq!(s.last_outcome.as_deref(), Some("rejected"));
        assert_eq!(s.last_code, Some(401));
        assert_eq!(s.last_stored, None);
    }

    #[test]
    fn sends_keep_their_slots_when_a_send_is_slow() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::from_secs(4));
        assert_eq!(run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(3)), 0);
        let at: Vec<u64> = sender.sent_at.borrow().iter().map(|d| d.as_secs()).collect();
        assert_eq!(at, [0, 900, 1800]);
    }

    #[test]
    fn a_wall_clock_jump_does_not_move_the_schedule() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        // NTP sets the wall clock back a year during the first send.
        struct Jumping<'a>(FakeSender<'a>);
        impl Sender for Jumping<'_> {
            fn send(&self, body: &str) -> SendOutcome {
                let outcome = self.0.send(body);
                self.0.clock.wall.set(self.0.clock.wall.get() - 31_536_000);
                outcome
            }
        }
        let sender = Jumping(FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::ZERO));
        run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(2));
        let at: Vec<u64> = sender.0.sent_at.borrow().iter().map(|d| d.as_secs()).collect();
        assert_eq!(at, [0, 900]);
    }

    #[test]
    fn a_missed_slot_is_sent_at_once_and_not_twice() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::from_secs(2000));
        run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(3));
        let at: Vec<u64> = sender.sent_at.borrow().iter().map(|d| d.as_secs()).collect();
        // The first send ends at 2000 s, inside slot 2: that slot goes at once.
        assert_eq!(at, [0, 2000, 4000]);
    }

    #[test]
    fn the_status_follows_each_send() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let answers = vec![SendOutcome::Stored, SendOutcome::Unreachable(7)];
        let sender = FakeSender::new(&clock, answers, Duration::ZERO);
        run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(3));
        let s = status::read(&t.root).unwrap();
        assert_eq!(s.device_id.as_deref(), Some("e8:f6:d7:00:17:df"));
        assert_eq!(s.collector, "https://diag.example.com/d");
        assert_eq!((s.sent, s.failed), (1, 2));
        assert_eq!(s.last_outcome.as_deref(), Some("unreachable"));
        assert_eq!(s.last_code, Some(7));
    }

    #[test]
    fn the_status_never_holds_the_token() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::ZERO);
        run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(1));
        let text = std::fs::read_to_string(t.root.join("status.json")).unwrap();
        assert!(!text.contains("SuperSecretKey"));
    }

    #[test]
    fn the_record_sent_is_the_dk_record() {
        let (t, clock) = (dk::tree(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::ZERO);
        run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(1));
        let body = sender.bodies.borrow()[0].clone();
        assert!(body.starts_with(r#"{"deviceId":"e8:f6:d7:00:17:df","timestamp":"2026-11-04T21:55:17Z","#));
        assert!(body.ends_with(r#""usb_pd_w":12.400}"#));
    }

    #[test]
    fn no_eth0_mac_stops_the_loop_with_the_reason() {
        let (t, clock) = (Tree::new(), FakeClock::new());
        let sender = FakeSender::new(&clock, vec![SendOutcome::Stored], Duration::ZERO);
        assert_eq!(run(&settings(), &t.fs(), &sender, &clock, &t.root, Some(1)), 1);
        assert!(sender.sent_at.borrow().is_empty());
        let s = status::read(&t.root).unwrap();
        assert_eq!(s.device_id, None);
        assert_eq!(s.error.as_deref(), Some("cannot read /sys/class/net/eth0/address"));
    }
}
