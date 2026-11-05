# verso-plugin-diag

A Verso plugin for the Mono Gateway. It reads the board's sensors every 15
minutes and sends one JSON record to `mono-collector` over HTTPS, with the
same keys as `mono-diagnostics-collector/thermal_logger.sh`. System › Sensor
upload shows the device ID and what the last send did.

- Design, decisions and evidence: [PLAN.md](PLAN.md)
- What to send to clients: [CLIENT.md](CLIENT.md)

## Layout

| Path | What it is |
|---|---|
| `src/main.rs` | `verso-plugin-diag` serves the page; `verso-plugin-diag push` runs the loop |
| `src/sensors.rs`, `src/record.rs` | the 17 sensors and the record |
| `src/http.rs` | the send, through `/usr/bin/curl` with the token on stdin |
| `src/push.rs`, `src/status.rs` | the loop and the status file the page reads |
| `src/page.rs` | the read-only page |
| `rootfs/etc/init.d/verso-plugin-diag` | procd instances `ui` and `push`, account `verso-plugin-diag` (UID 6100) |
| `packaging/` | the apk scripts and the config generator |

## Build and test

Needs Rust (`rustup`, the toolchain Verso pins), a Verso checkout at
`../verso` for the SDK, and for packaging the OpenWrt buildroot at
`../openwrt` (its `apk` tool and signing key) and `fakeroot`.

```sh
make test
make apk COLLECTOR_URL=https://diag.example.com/d DIAG_TOKEN=<token>
```

`make apk` writes `build/verso-plugin-diag-<version>.apk` for `aarch64_generic`.
Add `COLLECTOR_CA=path/to/ca.pem` when the collector's certificate is not from
a CA in the router's `ca-bundle`. Set `VERSION` and `RELEASE` for a new build.

The package holds the token. Treat every copy of it as holding the token.

## On the router

- Status: `cat /var/run/verso-plugin-diag/status.json`
- Log: `logread -e verso-plugin-diag`
- Settings: `/etc/config/verso_diag` (root only). After an edit, run
  `uci commit verso_diag && /etc/init.d/verso-plugin-diag restart`.
  (`reload_config` also works, but not the first time after the install: it
  only records the new file then.)
