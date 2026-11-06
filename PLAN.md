# verso-plugin-diag: plan

Status: 2026-11-04. Implemented and tested on the DK with Verso 0.11.0-r6 (sections 11 and 12). Open: deploy the collector on a public HTTPS name; fill the placeholders in CLIENT.md.

Goal: a Verso plugin that does what `mono-diagnostics-collector/thermal_logger.sh`
does. It reads the Mono Gateway sensors every 15 minutes and sends one JSON record
to `mono-collector` (`POST https://HOST/d`, header `X-Diag-Token`). A read-only
Verso page shows the device ID and the upload status.

The plugin runs on client routers for about one month, then it is removed.
Keep it small.

## 1. Glossary

| Term | Meaning |
|---|---|
| Shell | The Verso Go web server. It renders pages from JSON that plugins send. |
| Plugin | A separate program on a unix socket. It sends a widget schema (JSON), never HTML. |
| Envelope | The JSON reply of a plugin: `schema_version`, `title`, `widget`. |
| UCI | OpenWrt config system, files in `/etc/config/`. |
| procd | OpenWrt service supervisor. It starts, restarts and stops plugin processes. |
| Instance | One process that procd supervises. One init script can open more than one instance. |
| hwmon | Linux sensor interface in `/sys/class/hwmon/hwmonN/`. |
| of_node | The device-tree node of a sensor. Its path is stable across boots; `hwmonN` is not. |
| Config history | Verso's record of UCI changes (ADR-021). |
| ADR | Architecture decision record, `verso/docs/ADR/NNN-*.md`. |

## 2. Facts from the sources

thermal_logger.sh:

- 19 fields per record: `deviceId`, `timestamp`, 5 LS1046A TMU zones, 3 GPY PHYs,
  4 TMP431 channels, 2 SFP+ modules, fan RPM, fan PWM, USB PD power (W).
- Lookup methods: hwmon `name`, hwmon `label`, of_node path suffix, XFI cage match.
- Scaling: millidegrees to °C, microwatts to W, 3 decimals. Missing value is `null`.
- `deviceId` is `/sys/class/net/eth0/address`, read once. Empty value stops the logger
  (exit 1). `DEVICE_ID` env overrides it.
- Send timeout is 5 s. A failed send logs to stderr and the loop continues.
- Known defect: `DEVICE_ID` from env goes into the JSON without escaping, so a
  value with `"` makes the record invalid. The plugin validates it.

Verso:

- Plugins run as their own account `verso-plugin-<id>`, group `verso`, never root
  (ADR-007). The shell talks only to a socket that this account owns.
- A manifest needs at least one of `nav`, `entity_tabs`, `entity_acts`, `contributes`.
- A plugin with no `acl.write` gets no POST from the shell.
- Optional plugins are their own repositories (`verso-plugin-dnsdhcp`, `-ddns`, …),
  checked out into `verso/plugins/` for `make dev`.
- The Rust SDK `verso-plugin` gives `serve(id, get, post)`, `Request`, `Widget`,
  `Envelope`. Release profile: `opt-level="z"`, LTO, `panic="abort"`.

## 3. Decisions

| # | Decision |
|---|---|
| D1 | The record uses the same 19 keys (deviceId, timestamp and 17 sensors), in the same order, as thermal_logger.sh. |
| D2 | The loop is a second procd instance `push`: same binary, plugin account, not root. |
| D3 | Replaced by D9. |
| D4 | The plugin id is `diag`. The page is at System › "Mono feedback" (first "Sensor upload"; renamed 2026-11-05). |
| D5 | The interval is 900 s, a constant in the code. It cannot be changed. |
| D6 | There is no print-to-stdout mode (no equivalent of `-p`). |
| D7 | The UI is one read-only page: device ID, collector, last send, result, counts. There is no form. |
| D8 | The collector URL and the token are set at build time. `make apk` writes them into the default `/etc/config/verso_diag`. |
| D9 | HTTPS only. The plugin runs `/usr/bin/curl`; the apk depends on `curl` and `ca-bundle`. No TLS crate in Rust. |
| D10 | One shared token for all devices (no enrollment, no per-device token). |
| D11 | No MAC allowlist on the collector. |
| D13 | Sign with the existing buildroot key on this machine (`openwrt/private-key.pem`, fingerprint `74470ec683ee9eb5`). The client note (N1) adds its public key with one `wget` line. Interim: change it later if the Verso feed takes the plugin. |
| D12 | No end date. The plugin runs until the client removes it. Clients get written instructions (ticket N1). |

**Accepted risk (D8, D10):** the apk goes to clients, so the token can become
public. Then anyone can send records to the collector, and the collector cannot
tell them from real ones. HTTPS keeps the token and the records secret on the
network path. A leaked token is replaced only by a new apk on every device.

**Before shipping (GDPR):** tell each client what is collected (sensor values,
eth0 MAC), how long it is kept, and how to stop it (`apk del verso-plugin-diag`).
Do not log client IP addresses in the collector or in a TLS proxy.

## 4. Architecture

```
  make apk COLLECTOR_URL=https://… DIAG_TOKEN=…
        │ writes
        ▼
  /etc/config/verso_diag   (root-owned, 0600)
        │ init script reads it as root
        ▼
procd ── instance "ui"   : verso-plugin-diag          ← shell (unix socket, GET only)
     └── instance "push" : verso-plugin-diag push     → curl → mono-collector (HTTPS POST /d)
                               │ writes
                               ▼
               /var/run/verso-plugin-diag/status.json  (the "ui" instance reads it)
```

- Both instances run as `verso-plugin-diag:verso`. The directory
  `/var/run/verso-plugin-diag/` (0750) holds the socket and `status.json`.
- The init script opens the `push` instance only when `url` and `token` are not
  empty. It gives the settings to `push` as env.
- `procd_add_reload_trigger verso_diag` restarts `push` after an edit over SSH
  (`uci set` + `uci commit` + `reload_config`).
- The page reads only `status.json`. It needs no `acl` and no rpcd ACL file.

### UCI config `verso_diag` (generated by `make apk`)

```
config collector 'collector'
	option url '<COLLECTOR_URL>'     # https://name[:port]/d
	option token '<DIAG_TOKEN>'
	option ca ''                     # set by COLLECTOR_CA; empty = ca-bundle
	option device_id ''      # empty = eth0 MAC; set over SSH for a lab override
```

`make apk` stops with an error when `COLLECTOR_URL` is empty or does not start
with `https://`, or when `DIAG_TOKEN` is empty or has a character outside
`A-Za-z0-9._~+/=-`. apk writes the file again on each upgrade (section 11).

### Env given to `push`

`VERSO_DIAG_URL`, `VERSO_DIAG_TOKEN`, `VERSO_DIAG_DEVICE_ID`.
Only the account and root can read `/proc/<pid>/environ`.

## 5. Repository layout

```
verso-plugin-diag/
  Cargo.toml
  Makefile                 build, apk (with COLLECTOR_URL, DIAG_TOKEN)
  manifest.json            nav only, no acl
  i18n/sl.json
  rootfs/etc/init.d/verso-plugin-diag
  rootfs/etc/config/verso_diag.in   template that make apk fills
  src/main.rs       argv dispatch: (none) = page, "push" = loop
  src/sysfs.rs      SysFs trait, real and test-tree implementations
  src/sensors.rs    sensor table and lookup
  src/record.rs     Record type and JSON encoding
  src/device_id.rs  device ID source and check
  src/settings.rs   push settings from env
  src/http.rs       Sender trait, curl implementation
  src/status.rs     status file write and read
  src/push.rs       the loop
  src/page.rs       GET handler
  testdata/sysfs-dk/   a copy of the DK sysfs tree (from ticket I1)
```

## 6. Module contracts

### sysfs.rs

```rust
pub trait SysFs {
    /// Contents of a file, trailing newline removed. None if missing or unreadable.
    fn read(&self, path: &str) -> Option<String>;
    /// Canonical target of a link (like `readlink -f`). None if it cannot resolve.
    fn canonical(&self, path: &str) -> Option<String>;
    /// Entries of a directory that match `prefix`, sorted. Empty if missing.
    fn list(&self, dir: &str, prefix: &str) -> Vec<String>;
}
pub struct RealSysFs;                  // reads "/"
pub struct DirSysFs { root: PathBuf }  // reads a test tree under `root`
```

### sensors.rs

```rust
pub enum Locator {
    HwmonName(&'static str),   // /sys/class/hwmon/*/name equals
    HwmonLabel(&'static str),  // /sys/class/hwmon/*/label equals
    OfSuffix(&'static str),    // canonical(device/of_node) ends with
    Xfi(XfiCage),              // thermal_logger hwmon_matches_xfi rules
}
pub enum Scale { MilliToUnit, MicroToUnit, Raw }
pub struct SensorSpec { key: &'static str, locator: Locator, attribute: &'static str, scale: Scale }

/// The 17 sensor fields, in thermal_logger.sh order.
pub const GATEWAY_DK: &[SensorSpec];

/// First hwmon (in sorted hwmonN order) that matches `locator` and has `attribute`.
/// Returns the raw text of the attribute.
pub fn locate(fs: &dyn SysFs, locator: &Locator, attribute: &str) -> Option<String>;

/// Raw text to value. Integer text only (optional leading '-'); else None.
/// Raw requires a non-negative integer.
pub fn scale(raw: &str, scale: &Scale) -> Option<f64>;
```

### record.rs

```rust
pub struct Record { device_id: DeviceId, timestamp: String, values: Vec<(&'static str, Option<f64>)> }

/// Reads every spec once. Never fails: a missing sensor is None.
pub fn sample(fs: &dyn SysFs, specs: &[SensorSpec], device_id: &DeviceId, now: SystemTime) -> Record;

/// One-line JSON. Key order: deviceId, timestamp, then specs order.
/// Scaled values print with 3 decimals; Raw values print as integers; None prints null.
pub fn to_json(record: &Record) -> String;
```

### device_id.rs

```rust
pub struct DeviceId(String);   // always "xx:xx:xx:xx:xx:xx", lowercase
/// Override if non-empty, else /sys/class/net/eth0/address.
/// Err if the result is empty or not a MAC address.
pub fn resolve(fs: &dyn SysFs, override_value: &str) -> Result<DeviceId, DeviceIdError>;
```

### settings.rs

```rust
pub struct PushSettings { url: String, token: String, device_override: String }
/// Reads VERSO_DIAG_* from `env`. Err names the first missing or bad variable.
/// url must start with "https://". token must match the D8 character set, so it
/// is safe inside a curl config line.
pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<PushSettings, SettingsError>;
```

### http.rs

```rust
pub enum SendOutcome { Stored, Rejected(u16), Unreachable, Tls, Timeout, Failed(i32) }
pub trait Sender { fn send(&self, body: &str) -> SendOutcome; }

/// Runs one program with argv and stdin; returns exit code and stdout.
/// The seam that lets tests run without curl.
pub trait Runner { fn run(&self, exe: &str, argv: &[&str], stdin: &str) -> (i32, String); }

/// Writes `body` to <status_dir>/record.json (mode 0600), then runs
///   /usr/bin/curl -K - -sS -m 10 --proto =https -o /dev/null -w %{http_code}
///                 --data-binary @<status_dir>/record.json
/// with this config on stdin, so the token is never in argv (`ps` shows argv):
///   url = "<url>"
///   header = "X-Diag-Token: <token>"
///   header = "Content-Type: application/json"
/// Outcome: exit 0 and 204 = Stored; exit 0 and other code = Rejected(code);
/// exit 6 or 7 = Unreachable; exit 35 or 60 = Tls; exit 28 = Timeout;
/// any other exit = Failed(code).
pub struct CurlSender { url: String, token: String, status_dir: PathBuf, runner: Box<dyn Runner> }
```

### status.rs

```rust
pub struct Status {
    device_id: Option<String>,   // None when resolve failed
    collector: String,           // the URL, never the token
    error: Option<String>,       // why push stopped, e.g. "cannot read eth0 MAC"
    last_attempt: Option<u64>,   // epoch seconds
    last_outcome: Option<String>,
    last_stored: Option<u64>,
    sent: u64,
    failed: u64,
}
/// Writes to a temporary name in the same directory, then renames. Mode 0640.
pub fn write(dir: &Path, status: &Status) -> io::Result<()>;
/// None if the file is missing or does not parse.
pub fn read(dir: &Path) -> Option<Status>;
```

### push.rs

```rust
pub const INTERVAL: Duration = Duration::from_secs(900);
pub trait Clock { fn now(&self) -> SystemTime; fn sleep_until(&self, t: SystemTime); }
/// One cycle: sample, send, update status. Returns the new status.
pub fn tick(fs: &dyn SysFs, sender: &dyn Sender, clock: &dyn Clock, id: &DeviceId, prev: Status) -> Status;
/// Writes a first status (device ID, collector) before the first tick.
/// Runs tick() at start + n * INTERVAL, so the period does not drift.
/// A send failure logs one line to stderr and the loop continues.
/// A device ID failure writes `error` to the status and exits 1.
pub fn run(settings: PushSettings, fs: &dyn SysFs, sender: &dyn Sender, clock: &dyn Clock, status_dir: &Path) -> !;
```

### page.rs

```rust
/// GET "/": one properties card from status.json.
/// Any other path: Envelope::not_found().
pub fn get(req: &Request, status_dir: &Path) -> Envelope;
```

Page rows: Device ID (mono), Collector (mono), Last attempt, Last result, Last
stored, Sent, Failed. No status file reads as one callout: "The upload is not
running." A status with `error` shows the error in a danger callout.

## 7. Work items

These are rows of this plan, not tickets in a tracker.

| ID | Item | Estimate |
|---|---|---|
| I1 | Done. On the DK, as a non-root user: dump hwmon names, labels, of_node paths, and read each of the 17 attributes. | 30 min |
| I2 | Done. Find how an optional plugin repo (`verso-plugin-ddns`) builds and signs its apk, and which UID is free (core uses 6005 to 6007). Ask the Verso maintainer. | 30 min |
| I3 | Done. Confirm that the plugin account can open an outbound TCP connection on the DK (no procd jail, firewall OUTPUT accepts). | 10 min |
| I5 | Done (steps 1 to 3; step 4 against a local TLS collector). Check that `apk add curl` works on a client-type router (OpenWrt 25.12 feed) and that `curl https://<collector>/d` verifies the certificate with `ca-bundle`. | 15 min |
| I4 | Done. Find whether config history (ADR-021) records `/etc/config/verso_diag` and its token. If yes, move the settings to a non-UCI root-only file. | 15 min |
| T1 | Done. Repo skeleton: Cargo.toml, Makefile with the build variables, manifest, init script with the `ui` instance only. GET shows the page (then "Sensor upload") with an empty card. | 1.5 h |
| T2 | Done. Build `testdata/sysfs-dk/` from `investigations/I1-sysfs-dump.txt`, then `sysfs.rs` + `sensors.rs` + `record.rs` + `device_id.rs`. | 3 h |
| T3 | Done. `http.rs` + `status.rs` + `settings.rs` + `push.rs`, and the `push` instance in the init script. | 2.5 h |
| N1 | Done. Client note: what is collected (sensor values, eth0 MAC), why, for how long, how to add the signing key (`wget -O /etc/apk/keys/mono-diag.pem <url>`), how to install, how to remove (`apk del verso-plugin-diag`), and that removal stops all sending. | 30 min |
| C1 | Code done (`-cert`/`-key`); the public name and certificate are open. Collector: serve HTTPS (Go `ListenAndServeTLS` with `-cert`/`-key`, or a TLS proxy) on a public name with a certificate from a public CA. Store no client IPs. | 1 h |
| T4 | Done. `page.rs` and `i18n/sl.json`. | 1 h |
| T5 | Done but one check: the page inside the signed-in shell (needs the root password). End-to-end on the DK against `mono-collector`: compare one plugin record with one `thermal_logger.sh -p` record taken at the same time. | 1 h |

### T1 deliverables

| File | Content |
|---|---|
| `Cargo.toml` | crate `verso-plugin-diag`; deps `verso-plugin` (path `../verso-plugin-sdk`), `serde_json`; release profile as the core plugins |
| `manifest.json` | id `diag`, socket `/var/run/verso-plugin-diag/diag.sock`, `schema_version` 1, nav `{section: System, label: Mono feedback, path: /}`, no `acl` |
| `rootfs/etc/init.d/verso-plugin-diag` | account `verso-plugin-diag:x:6100:6000`, socket dir 0750, named instance `ui`, env `VERSO_DIAG_SOCKET` |
| `rootfs/etc/config/verso_diag.in` | the UCI template from section 4 |
| `packaging/post-install.sh`, `pre-deinstall.sh`, `post-deinstall.sh` | as section 10 |
| `Makefile` | `build` (arm64 musl), `apk` (checks `COLLECTOR_URL` and `DIAG_TOKEN`, fills the template, `mkpkg`, signs with `KEY ?= ../../../openwrt/private-key.pem`), `test` |
| `src/main.rs` | `serve("diag", get, post)`; GET `/` returns an empty card titled "Mono feedback"; any other path `not_found` |

### T1 test cases

- `make apk` without `DIAG_TOKEN` stops with an error and writes no package.
- `make apk COLLECTOR_URL=http://x/d DIAG_TOKEN=x` stops with an error (not https).
- `make apk DIAG_TOKEN='a"b' COLLECTOR_URL=https://x/d` stops with an error (bad character).
- `make apk COLLECTOR_URL=https://diag.example.com/d DIAG_TOKEN=abc` gives a config
  with that `url` and `token`.
- The init script with an empty `token` opens only the `ui` instance.
- The init script stops with an error when UID 6100 belongs to another account.
- `apk del verso-plugin-diag` stops both instances (pre-deinstall runs the init
  script `stop`), removes the nav row (post-deinstall sends SIGHUP to the shell),
  and `pgrep -f verso-plugin-diag` then finds nothing.
- `curl --unix-socket /var/run/verso-plugin-diag/diag.sock http://x/` returns an
  envelope with `title` "Mono feedback".

### T2 test cases (function level)

`scale`:
- `"61250"`, MilliToUnit → `Some(61.25)`.
- `"-5000"`, MilliToUnit → `Some(-5.0)`.
- `"2500000"`, MicroToUnit → `Some(2.5)`.
- `"1200"`, Raw → `Some(1200.0)`; `"-1"`, Raw → `None`.
- `""`, `"abc"`, `"12.5"` → `None` for each scale.

`locate`:
- HwmonName `ddr_thermal` finds the hwmon whose `name` matches and returns `temp1_input`.
- Two hwmon dirs match; the first in lexical order wins, like the script glob
  (`hwmon10` comes before `hwmon2`).
- A match without the attribute is skipped; the next match is used.
- OfSuffix `/i2c@1/temp-sensor@4c` with `temp2_input` returns the remote TMP431 value.
- OfSuffix `/ethernet-phy@1` does not match `.../ethernet-phy@1/child`.
- Xfi `xfi0` matches by `name` `sfp_xfi0`, by device path `sfp_xfi0`, and by
  of_node `ethernet@f0000`; `xfi1` matches by `ethernet@f2000`.
- No hwmon directory → `None`.

`device_id::resolve`:
- Override `AA:BB:CC:DD:EE:FF` → `aa:bb:cc:dd:ee:ff`.
- Override `aa:bb"` → `Err`.
- No override, file `aa:bb:cc:dd:ee:ff\n` → `Ok`.
- No override, file missing or empty → `Err`.

`to_json`:
- All values None → every sensor key is `null`; output parses as JSON.
- Key order equals thermal_logger.sh order (golden string).
- `61.25` prints `61.250`; fan RPM `1200` prints `1200`.
- `sample` on `testdata/sysfs-dk/` with a fixed time equals a golden line written
  from the I1 values.

### T3 test cases (outline)

- `CurlSender` with a fake `Runner`: (0, "204") → Stored; (0, "401") → Rejected(401);
  exit 7 → Unreachable; exit 60 → Tls; exit 28 → Timeout; exit 2 → Failed(2).
- The fake `Runner` sees: no token in argv; the token header and the url on stdin;
  `--proto =https` in argv; `record.json` holds the exact body, mode 0600.
- One manual check against the real collector over HTTPS (in T5): 204 is Stored,
  a wrong token is Rejected(401).
- `tick` with a fake sender: the counters and `last_outcome` change correctly.
- `run` with a fake clock: three ticks are at start, start+900 s, start+1800 s,
  also when a send takes 4 s.
- `run` with no eth0 MAC: the status has `error`, and the exit code is 1.
- `from_env`: missing token → Err naming `VERSO_DIAG_TOKEN`; url `http://x/d` → Err;
  token with a space or `"` → Err.
- `status::write` then `read` returns the same value; a broken file reads as None.
- The status file never contains the token.

### T4 test cases (outline)

- No status file → one callout "The upload is not running."
- Status with `error` → a danger callout with the error text.
- Normal status → 7 property rows; Device ID and Collector are mono.
- GET `/other` → `not_found`.

## 8. Verification evidence

| Item | Evidence | Result |
|---|---|---|
| I1 | `investigations/I1-sysfs-dump.txt` (2026-11-04, DK, OpenWrt 25.12.5, kernel 6.12.103). All 17 attributes and `eth0/address` read as `nobody` (uid 65534) through `start-stop-daemon -c nobody`. Modes are `r--r--r--`, `pwm1` is `rw-r--r--`. Mapping in section 9. | Pass |
| I2 | From the Verso source and git history, 2026-11-04. Section 10. | Pass |
| I3 | Firewall `defaults.output` is `ACCEPT`; fw4 `output` chain policy accept. 2026-11-04: `curl` as `nobody` (uid 65534) from the DK to `http://192.168.99.201:8888/d` with the token returned `204`. Verso is not installed on this DK, so no plugin jail exists to test; Verso's plugin init scripts use no jail. | Pass |
| I4 | Read from the Verso source, 2026-11-04. (1) `history.db`: the shell keeps a reading only for a config in `shellConfigs` (`verso`, `system`) or in a manifest's `uci` acl (`internal/server/config_history_direct.go:166`, `staged.go:233`). This plugin declares no acl, so `verso_diag` is never stored. (2) The spool: `verso-rpcd` reads every file in `/etc/config` (`verso-rpcd/src/config_watch.rs:331`). It writes it, with the token in plain text, to `/var/run/verso/changes/` (dir `0700`, owner `verso`; file `0644`). The shell deletes it within 2 s. This happens on each write of the file and on each helper start (boot). Plugin accounts cannot read the directory. Recommended (wait for confirmation): keep UCI (D8) and accept that the shell account sees the token for up to 2 s. | Pass |
| I5 | DK, 2026-11-04: `curl 8.21.0` with mbedTLS 3.6.6 (packages `curl` 160 KiB + `libcurl4` 526 KiB). `curl -K -` with the URL on stdin and `--proto =https` returned 200 from `https://sysupgrade.mono.si`. `https://self-signed.badssl.com/` gave exit 60. | Pass |
| T1 | `make apk COLLECTOR_URL=https://diag.example.com/d DIAG_TOKEN=dummy-token` built and signed `build/verso-plugin-diag-0.1.0-r0.apk` (328 KB). `apk adbdump`: all paths root:root, directories 0755, `/etc/config/verso_diag` 0600, depends `verso curl ca-bundle`. `sh -n` passes on the init script and package scripts. `packaging/make-config.test.sh`: 8 checks pass. | Pass |
| T2 to T4 | `cargo test --locked`: 63 tests pass. arm64 release binary: 571 KiB, static. | Pass |
| T5 | Section 11. | Pass without the shell |

Format check, 2026-11-04: a `thermal_logger.sh` record already in the host
`diag.jsonl` has the same keys and the 3-decimal format as section 9, and
`tmp_u58001_remote_c` is `0.000` there too.

## 9. I1 results: sensor mapping on the DK

| Record key | Locator | hwmon on DK | Sample raw | Value |
|---|---|---|---|---|
| ls1046a_ddr_controller_c | name `ddr_thermal` | hwmon6 | 45000 | 45.000 |
| ls1046a_serdes_c | name `serdes_thermal` | hwmon7 | 44000 | 44.000 |
| ls1046a_fman_c | name `fman_thermal` | hwmon8 | 46000 | 46.000 |
| ls1046a_a72_cluster_c | name `cluster_thermal` | hwmon9 | 46000 | 46.000 |
| ls1046a_sec_c | name `sec_thermal` | hwmon10 | 44000 | 44.000 |
| gpy_u11000_c | of `/ethernet-phy@0` | hwmon0 | 44237 | 44.237 |
| gpy_u12000_c | of `/ethernet-phy@1` | hwmon1 | 48882 | 48.882 |
| gpy_u13000_c | of `/ethernet-phy@2` | hwmon2 | 47347 | 47.347 |
| tmp_u58000_local_c | of `/i2c@1/temp-sensor@4c` temp1 | hwmon4 | 40500 | 40.500 |
| tmp_u58000_remote_c | of `/i2c@1/temp-sensor@4c` temp2 | hwmon4 | 47063 | 47.063 |
| tmp_u58001_local_c | of `/i2c@2/temp-sensor@4c` temp1 | hwmon5 | 47375 | 47.375 |
| tmp_u58001_remote_c | of `/i2c@2/temp-sensor@4c` temp2 | hwmon5 | 0 | 0.000 |
| sfp_p16000_c | xfi0 (name `sfp_xfi0`) | hwmon11 | 47219 | 47.219 |
| sfp_p17000_c | xfi1 (name `sfp_xfi1`) | hwmon12 | 46699 | 46.699 |
| fan1_rpm | name `emc2305` fan1_input | hwmon3 | 2558 | 2558 |
| fan1_pwm | name `emc2305` pwm1 | hwmon3 | 76 | 76 |
| usb_pd_w | label `USB Power Delivery` power1 | hwmon13 | 12400000 | 12.400 |

Facts for the code:

- Every locator finds exactly one hwmon. No name or of_node suffix is shared.
- `tmp_u58001_remote_c` reads `0`: the remote diode is not connected (the board
  profile says the same). D1 keeps it, so the record says `0.000`, not `null`.
- The thermal-zone hwmons (hwmon6 to hwmon10) have no `device/of_node` link.
  `canonical()` must return None for them, and OfSuffix must skip them.
- The GPY hwmon names are bus IDs (`0x0000000001afd000:00`). Only of_node finds them.
- Eight `ina234` share the name. Only the label finds the USB PD rail.
- `eth0` MAC on this DK: `e8:f6:d7:00:17:df`.

## 10. I2 results: packaging, UID, signing

Packaging (from `verso/Makefile` and the `apk-qos` target before commit
`370110d2`, when optional plugins left the Verso repo):

- Build: `cargo build --locked --release --target aarch64-unknown-linux-musl`.
  Only arm64: the sensors exist only on the Mono Gateway.
- The SDK is a path dependency, `verso-plugin = { path = "../verso-plugin-sdk" }`.
  The repo is checked out at `verso/plugins/verso-plugin-diag/` (gitignored there),
  and `scripts/dev.sh` deploys every `plugins/verso-plugin-*/` in `make dev`.
- Payload: `/usr/bin/verso-plugin-diag`, `/etc/init.d/verso-plugin-diag`,
  `/etc/config/verso_diag`, `/usr/share/verso/plugins/diag/manifest.json`,
  `/usr/share/verso/plugins/diag/i18n/sl.json`. All owned `0:0` (fakeroot).
- `apk mkpkg --info name:verso-plugin-diag --info version:<v> --info arch:aarch64_generic
  --info "depends:verso curl ca-bundle" --script post-install:… --script post-upgrade:…
  --script pre-deinstall:… --script post-deinstall:… --sign-key <KEY>`.
  The apk tool is `openwrt/staging_dir/host/bin/apk` (present on this machine).
- post-install: add the account, enable and restart the service, then
  `ubus call service signal '{"name":"verso","signal":1}'` (docs/plugins.md).
- pre-deinstall: stop and disable the service.
- post-deinstall: remove the passwd line and `/var/run/verso-plugin-diag`, then
  the same SIGHUP to the shell. Each plugin package removes its own user.

UID:

- Used: 6000 `verso`; 6005 system, 6006 firewall, 6007 interfaces (core);
  6009 qos, 6010 vpn (history); 6011 dnsdhcp (`internal/plugin/accounts_test.go`).
  acme and ddns are not in this repo, so their UIDs are unknown (6008 is a gap).
- The shell maps `verso-plugin-<id>` to a UID through `/etc/passwd` and compares it
  with the socket owner (`internal/plugin/accounts.go`). Two accounts with one UID
  could write in each other's socket directory, so the UID must be unique.
- History shows one collision: before `370110d2`, dnsdhcp and interfaces both used 6007.
- Use **6100** for `verso-plugin-diag`, away from the sequence. The init script
  stops with an error when another account already has 6100.

Signing (public key fingerprints, SHA-256 of DER, first 16 hex digits):

| Key | Fingerprint |
|---|---|
| `openwrt/public-key.pem` on this machine | `74470ec683ee9eb5` |
| Verso feed key `https://sysupgrade.mono.si/verso/verso.pem` | `92425bc8d289084e` |
| DK firmware key `/etc/apk/keys/public-key.pem` | `c433c5ac3eb0a5c1` |

All three are different. A package signed on this machine is not trusted by a
client router. D13 options:

1. Own key (this machine's buildroot key or a new one). The client note (N1) adds
   one line: `wget -O /etc/apk/keys/mono-diag.pem <url>`. No dependency on others.
2. The Verso maintainer signs it with the feed key and publishes it in the Verso
   feed. Clients need nothing extra; the token is then on a public feed.
3. `apk add --allow-untrusted`. Rejected: it teaches clients to skip the check.

## 11. Implementation notes and T5 evidence (2026-11-04)

Changes from the plan made during the implementation:

- **17 sensors, 19 keys.** The plan said 16 and 18. The script has 17 sensor fields.
- **Repository place.** At first the repo was beside the Verso checkout, with the
  SDK at `../verso/plugins/verso-plugin-sdk`. Since 2026-11-06 it is checked out
  inside `verso/plugins/`, as planned, and the SDK path is `../verso-plugin-sdk`.
- **Clock.** The schedule uses a monotonic clock, and the records use the wall clock.
  So when NTP sets the clock after boot, no sends bunch up and none are skipped
  (test `a_wall_clock_jump_does_not_move_the_schedule`).
- **Optional CA.** `COLLECTOR_CA=ca.pem` ships a CA file and sets `option ca`, for a
  collector whose certificate is not from a public CA. Without it, `ca-bundle` is used.
- **Not a conffile.** apk v3 writes `/etc/config/verso_diag` again on each upgrade.
  The build writes the same settings, so nothing is lost.
- **Page.** The failed count is labelled "Not sent", because "Failed" is also an
  outcome word, and a catalog key has only one translation. A status with no attempt
  for two intervals shows a warning.
- **Body file.** The record goes to curl in `/var/run/verso-plugin-diag/record.json`
  (0600). The token goes on curl's stdin. On the DK, `ps` showed no token.
- **`$` is not allowed in the URL**, so the URL is safe in shell double quotes.

T5 on the DK (OpenWrt 25.12.5, Verso not installed), with a local `mono-collector`
on HTTPS (test CA) through an SSH reverse tunnel. The binary ran as `nobody`:

| Check | Result |
|---|---|
| `push` with the right token and the test CA | collector stored the record; `status.json` `sent 1`, outcome `stored` |
| `push` with a wrong token | `Rejected(401)`, `failed 1`, `last_code 401` |
| `push` without the CA (ca-bundle only) | `Tls` |
| `push` with `http://` | exit 1, status `error` "VERSO_DIAG_URL is not valid" |
| `ui` socket, GET `/` | envelope "Sensor upload" (the name then) with 7 rows (device ID `e8:f6:d7:00:17:df`) |
| `ui` socket, GET `/other` | `not_found: true` |
| Token in `ps` | not found |
| Memory | about 780 KB per process |
| Plugin record vs `thermal_logger.sh -p` (11 s apart) | same 19 keys in the same order, no nulls, values within sensor noise |

## 12. Install test on the DK with Verso (2026-11-04)

Verso 0.11.0-r6 from `https://sysupgrade.mono.si/verso/aarch64_generic/packages.adb`.
No backup was made first (the user chose to skip it). Verso listens on 8443;
uhttpd keeps 80 and 443. The test collector ran on this machine at
`https://127.0.0.1:9443/d` through an SSH reverse tunnel, with a test CA.

| Check | Result |
|---|---|
| `apk add` without `/etc/apk/keys/mono-diag.pem` | `UNTRUSTED signature`, not installed |
| `apk add` with the key | installed; post-install ran |
| Account | `verso-plugin-diag:x:6100:6000` |
| Run directory | `drwxr-x--- verso-plugin-diag:verso`; socket 0660, status 0640, record 0600 |
| procd | instances `ui` and `push` running as `verso-plugin-diag` |
| First send | at start; collector stored it; status `sent 1`, `stored` |
| Shell | log "rediscovered 4 plugin(s)" after the install, "3" after `apk del` |
| `apk del` | no process left; account, run directory, manifest, init script and config removed |
| UID 6100 taken by another account | no process, no account added; log `daemon.err verso-plugin-diag: uid 6100 belongs to another account; not starting` (r1; r0 did not log it) |
| `uci set …device_id` + `reload_config` | push restarted and sent as `aa:bb:cc:00:00:01`; reverting restarted it again |
| First `reload_config` after the install | no restart: OpenWrt's `reload_config` only records the hash of a config it has not seen. README says to restart the service. |
| `/etc/config/verso_diag` on the router | `-rw------- root root` |

Notes:

- The Verso README builds the feed URL from `apk --print-arch`, which prints
  `aarch64` on the DK; the feed is only at `aarch64_generic` (`aarch64` gives 404).
  `/etc/apk/arch` says `aarch64_generic`. CLIENT.md gives the full URL.
