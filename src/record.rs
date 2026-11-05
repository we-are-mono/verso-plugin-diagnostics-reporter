// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! One reading of every sensor, as the JSON line thermal_logger.sh sends.

use crate::device_id::DeviceId;
use crate::sensors::{locate, scale, SensorSpec, Value};
use crate::sysfs::SysFs;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub device_id: DeviceId,
    pub timestamp: String,
    pub values: Vec<(&'static str, Option<Value>)>,
}

/// Reads every spec once. A missing or unreadable sensor is None.
pub fn sample(fs: &dyn SysFs, specs: &[SensorSpec], device_id: &DeviceId, now: SystemTime) -> Record {
    let values = specs
        .iter()
        .map(|s| {
            let value = locate(fs, &s.locator, s.attribute).and_then(|raw| scale(&raw, &s.scale));
            (s.key, value)
        })
        .collect();
    Record { device_id: device_id.clone(), timestamp: utc(now), values }
}

/// One JSON line: deviceId, timestamp, then each value in spec order. A Fixed3
/// value has 3 decimals, a Count none, and a missing one is null.
pub fn to_json(record: &Record) -> String {
    // Every string here is a MAC, a timestamp or a key from the table, none of
    // which needs escaping.
    let mut out = format!(
        "{{\"deviceId\":\"{}\",\"timestamp\":\"{}\"",
        record.device_id.as_str(),
        record.timestamp
    );
    for (key, value) in &record.values {
        let text = match value {
            Some(Value::Fixed3(v)) => format!("{v:.3}"),
            Some(Value::Count(n)) => n.to_string(),
            None => "null".into(),
        };
        out.push_str(&format!(",\"{key}\":{text}"));
    }
    out.push('}');
    out
}

/// `t` in UTC as "YYYY-MM-DDTHH:MM:SSZ" (date -u '+%Y-%m-%dT%H:%M:%SZ').
pub fn utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The proleptic Gregorian date of a day count since 1970-01-01
/// (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_id::resolve;
    use crate::sensors::{dk, GATEWAY_DK};
    use crate::sysfs::tree::Tree;
    use std::time::Duration;

    // 2026-11-04T21:55:17Z, the time of the I1 dump.
    const AT: u64 = 1_793_829_317;

    fn at() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(AT)
    }

    #[test]
    fn the_dk_reads_as_the_script_writes_it() {
        let t = dk::tree();
        let id = resolve(&t.fs(), "").unwrap();
        let line = to_json(&sample(&t.fs(), GATEWAY_DK, &id, at()));
        assert_eq!(
            line,
            concat!(
                r#"{"deviceId":"e8:f6:d7:00:17:df","timestamp":"2026-11-04T21:55:17Z","#,
                r#""ls1046a_ddr_controller_c":45.000,"ls1046a_serdes_c":44.000,"#,
                r#""ls1046a_fman_c":46.000,"ls1046a_a72_cluster_c":46.000,"ls1046a_sec_c":44.000,"#,
                r#""gpy_u11000_c":44.237,"gpy_u12000_c":48.882,"gpy_u13000_c":47.347,"#,
                r#""tmp_u58000_local_c":40.500,"tmp_u58000_remote_c":47.063,"#,
                r#""tmp_u58001_local_c":47.375,"tmp_u58001_remote_c":0.000,"#,
                r#""sfp_p16000_c":47.219,"sfp_p17000_c":46.699,"#,
                r#""fan1_rpm":2558,"fan1_pwm":76,"usb_pd_w":12.400}"#
            )
        );
    }

    #[test]
    fn a_tree_with_no_sensors_reads_null_everywhere() {
        let t = Tree::new();
        t.file("/sys/class/net/eth0/address", "e8:f6:d7:00:17:df");
        let id = resolve(&t.fs(), "").unwrap();
        let line = to_json(&sample(&t.fs(), GATEWAY_DK, &id, at()));
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        let object = parsed.as_object().unwrap();
        assert_eq!(object.len(), 19);
        for spec in GATEWAY_DK {
            assert!(object[spec.key].is_null(), "{}", spec.key);
        }
    }

    #[test]
    fn keys_follow_the_table_order() {
        let t = dk::tree();
        let id = resolve(&t.fs(), "").unwrap();
        let record = sample(&t.fs(), GATEWAY_DK, &id, at());
        let keys: Vec<&str> = record.values.iter().map(|(k, _)| *k).collect();
        let table: Vec<&str> = GATEWAY_DK.iter().map(|s| s.key).collect();
        assert_eq!(keys, table);
    }

    #[test]
    fn fixed_values_have_three_decimals_and_counts_none() {
        let t = Tree::new();
        t.file("/sys/class/net/eth0/address", "e8:f6:d7:00:17:df");
        let id = resolve(&t.fs(), "").unwrap();
        let record = Record {
            device_id: id,
            timestamp: utc(at()),
            values: vec![("a", Some(Value::Fixed3(61.25))), ("b", Some(Value::Count(1200)))],
        };
        assert!(to_json(&record).ends_with(r#""a":61.250,"b":1200}"#));
    }

    #[test]
    fn utc_writes_the_date_the_way_date_does() {
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(utc(at()), "2026-11-04T21:55:17Z");
        // A leap day.
        assert_eq!(utc(UNIX_EPOCH + Duration::from_secs(1_709_208_000)), "2024-02-29T12:00:00Z");
    }
}
