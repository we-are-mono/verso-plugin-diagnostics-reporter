// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! The Mono Gateway sensors, found the way mono-diagnostics-collector's
//! thermal_logger.sh finds them, so a record from this plugin and one from the
//! script compare field by field.

use crate::sysfs::SysFs;

const HWMON: &str = "/sys/class/hwmon";

/// Which SFP+ cage an XFI locator names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum XfiCage {
    Xfi0,
    Xfi1,
}

/// How a sensor's hwmon directory is found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Locator {
    /// The hwmon's `name` equals this.
    HwmonName(&'static str),
    /// The hwmon's `label` equals this.
    HwmonLabel(&'static str),
    /// The canonical path of the hwmon's `device/of_node` ends with this.
    OfSuffix(&'static str),
    /// The hwmon belongs to this SFP+ cage (thermal_logger.sh hwmon_matches_xfi).
    Xfi(XfiCage),
}

/// How a raw sysfs reading becomes a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    /// Millidegrees Celsius to degrees, written with 3 decimals.
    MilliToUnit,
    /// Microwatts to watts, written with 3 decimals.
    MicroToUnit,
    /// A non-negative whole number (RPM, PWM duty), written as it is.
    Count,
}

/// A scaled reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Fixed3(f64),
    Count(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorSpec {
    /// The record's key.
    pub key: &'static str,
    pub locator: Locator,
    pub attribute: &'static str,
    pub scale: Scale,
}

const fn spec(key: &'static str, locator: Locator, attribute: &'static str, scale: Scale) -> SensorSpec {
    SensorSpec { key, locator, attribute, scale }
}

use Locator::{HwmonLabel, HwmonName, OfSuffix, Xfi};
use Scale::{Count, MicroToUnit, MilliToUnit};

/// The 17 sensor fields, in thermal_logger.sh order.
pub const GATEWAY_DK: &[SensorSpec] = &[
    // LS1046A thermal zones.
    spec("ls1046a_ddr_controller_c", HwmonName("ddr_thermal"), "temp1_input", MilliToUnit),
    spec("ls1046a_serdes_c", HwmonName("serdes_thermal"), "temp1_input", MilliToUnit),
    spec("ls1046a_fman_c", HwmonName("fman_thermal"), "temp1_input", MilliToUnit),
    spec("ls1046a_a72_cluster_c", HwmonName("cluster_thermal"), "temp1_input", MilliToUnit),
    spec("ls1046a_sec_c", HwmonName("sec_thermal"), "temp1_input", MilliToUnit),
    // GPY PHYs, by device-tree PHY address.
    spec("gpy_u11000_c", OfSuffix("/ethernet-phy@0"), "temp1_input", MilliToUnit),
    spec("gpy_u12000_c", OfSuffix("/ethernet-phy@1"), "temp1_input", MilliToUnit),
    spec("gpy_u13000_c", OfSuffix("/ethernet-phy@2"), "temp1_input", MilliToUnit),
    // TMP431 on I2C mux channels 1 and 2: temp1 local, temp2 the remote diode.
    spec("tmp_u58000_local_c", OfSuffix("/i2c@1/temp-sensor@4c"), "temp1_input", MilliToUnit),
    spec("tmp_u58000_remote_c", OfSuffix("/i2c@1/temp-sensor@4c"), "temp2_input", MilliToUnit),
    spec("tmp_u58001_local_c", OfSuffix("/i2c@2/temp-sensor@4c"), "temp1_input", MilliToUnit),
    spec("tmp_u58001_remote_c", OfSuffix("/i2c@2/temp-sensor@4c"), "temp2_input", MilliToUnit),
    // SFP+ modules; in the board DTS XFI0 is P16000 and XFI1 is P17000.
    spec("sfp_p16000_c", Xfi(XfiCage::Xfi0), "temp1_input", MilliToUnit),
    spec("sfp_p17000_c", Xfi(XfiCage::Xfi1), "temp1_input", MilliToUnit),
    // EMC2305 fan: tachometer and PWM duty (0-255).
    spec("fan1_rpm", HwmonName("emc2305"), "fan1_input", Count),
    spec("fan1_pwm", HwmonName("emc2305"), "pwm1", Count),
    // INA234 on the USB Power Delivery input.
    spec("usb_pd_w", HwmonLabel("USB Power Delivery"), "power1_input", MicroToUnit),
];

/// The raw text of `attribute` in the first hwmon, in glob order, that matches
/// `locator` and has a readable `attribute`.
pub fn locate(fs: &dyn SysFs, locator: &Locator, attribute: &str) -> Option<String> {
    fs.list(HWMON, "hwmon").into_iter().find_map(|entry| {
        let dir = format!("{HWMON}/{entry}");
        if !matches(fs, &dir, locator) {
            return None;
        }
        fs.read(&format!("{dir}/{attribute}"))
    })
}

fn matches(fs: &dyn SysFs, dir: &str, locator: &Locator) -> bool {
    match locator {
        HwmonName(name) => fs.read(&format!("{dir}/name")).as_deref() == Some(*name),
        HwmonLabel(label) => fs.read(&format!("{dir}/label")).as_deref() == Some(*label),
        OfSuffix(suffix) => fs
            .canonical(&format!("{dir}/device/of_node"))
            .is_some_and(|node| node.ends_with(suffix)),
        Xfi(cage) => matches_xfi(fs, dir, *cage),
    }
}

/// thermal_logger.sh hwmon_matches_xfi: the cage by the hwmon's name, else by
/// its device or device-tree path.
fn matches_xfi(fs: &dyn SysFs, dir: &str, cage: XfiCage) -> bool {
    let (names, needles): (&[&str], &[&str]) = match cage {
        XfiCage::Xfi0 => (&["sfp_xfi0", "xfi0"], &["sfp_xfi0", "/xfi0:", "ethernet@f0000"]),
        XfiCage::Xfi1 => (&["sfp_xfi1", "xfi1"], &["sfp_xfi1", "/xfi1:", "ethernet@f2000"]),
    };
    if let Some(name) = fs.read(&format!("{dir}/name")) {
        if names.contains(&name.as_str()) {
            return true;
        }
    }
    let device = fs.canonical(&format!("{dir}/device")).unwrap_or_default();
    let of_node = fs.canonical(&format!("{dir}/device/of_node")).unwrap_or_default();
    let paths = format!("{device}:{of_node}");
    needles.iter().any(|needle| paths.contains(needle))
}

/// The value of a raw reading. MilliToUnit and MicroToUnit take a whole number
/// with an optional leading '-'; Count takes digits only. Anything else is None.
pub fn scale(raw: &str, scale: &Scale) -> Option<Value> {
    match scale {
        MilliToUnit => whole(raw).map(|v| Value::Fixed3(v / 1_000.0)),
        MicroToUnit => whole(raw).map(|v| Value::Fixed3(v / 1_000_000.0)),
        Count => {
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            raw.parse().ok().map(Value::Count)
        }
    }
}

fn whole(raw: &str) -> Option<f64> {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

/// The DK's sensors as I1 found them (investigations/I1-sysfs-dump.txt), for
/// tests of this module and of the record.
#[cfg(test)]
pub mod dk {
    use crate::sysfs::tree::Tree;

    pub fn tree() -> Tree {
        let t = Tree::new();
        let dt = "/sys/firmware/devicetree/base/soc";
        let mut n = 0;
        let mut hwmon = |name: &str, of: Option<&str>, attrs: &[(&str, &str)]| {
            let dir = format!("/sys/class/hwmon/hwmon{n}");
            n += 1;
            t.file(&format!("{dir}/name"), name);
            for (attr, value) in attrs {
                t.file(&format!("{dir}/{attr}"), value);
            }
            t.dir(&format!("{dir}/device"));
            if let Some(of) = of {
                t.dir(of);
                t.link(&format!("{dir}/device/of_node"), of);
            }
        };
        let phy = format!("{dt}/fman@1a00000/mdio@fd000/ethernet-phy@");
        hwmon("0x0000000001afd000:00", Some(&format!("{phy}0")), &[("temp1_input", "44237")]);
        hwmon("0x0000000001afd000:01", Some(&format!("{phy}1")), &[("temp1_input", "48882")]);
        hwmon("0x0000000001afd000:02", Some(&format!("{phy}2")), &[("temp1_input", "47347")]);
        hwmon(
            "emc2305",
            Some(&format!("{dt}/i2c@2180000/i2c-mux@70/i2c@3/fan-controller@2e")),
            &[("fan1_input", "2558"), ("fan2_input", "0"), ("pwm1", "76"), ("pwm2", "76")],
        );
        hwmon(
            "tmp431",
            Some(&format!("{dt}/i2c@2180000/i2c-mux@70/i2c@1/temp-sensor@4c")),
            &[("temp1_input", "40500"), ("temp2_input", "47063")],
        );
        hwmon(
            "tmp431",
            Some(&format!("{dt}/i2c@2180000/i2c-mux@70/i2c@2/temp-sensor@4c")),
            &[("temp1_input", "47375"), ("temp2_input", "0")],
        );
        for (zone, value) in [
            ("ddr_thermal", "45000"),
            ("serdes_thermal", "44000"),
            ("fman_thermal", "46000"),
            ("cluster_thermal", "46000"),
            ("sec_thermal", "44000"),
        ] {
            hwmon(zone, None, &[("temp1_input", value)]);
        }
        hwmon("sfp_xfi0", Some("/sys/firmware/devicetree/base/sfp-xfi0"), &[("temp1_input", "47219")]);
        hwmon("sfp_xfi1", Some("/sys/firmware/devicetree/base/sfp-xfi1"), &[("temp1_input", "46699")]);
        for (i, (label, power)) in [("USB Power Delivery", "12400000"), ("5V PSU", "11120000")]
            .into_iter()
            .enumerate()
        {
            let of = format!("{dt}/i2c@21a0000/i2c-mux@70/i2c@0/power_sensor@4{i}");
            hwmon("ina234", Some(&of), &[("label", label), ("power1_input", power)]);
        }
        t.file("/sys/class/net/eth0/address", "e8:f6:d7:00:17:df");
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::tree::Tree;

    #[test]
    fn scale_turns_millidegrees_into_degrees() {
        assert_eq!(scale("61250", &MilliToUnit), Some(Value::Fixed3(61.25)));
        assert_eq!(scale("-5000", &MilliToUnit), Some(Value::Fixed3(-5.0)));
    }

    #[test]
    fn scale_turns_microwatts_into_watts() {
        assert_eq!(scale("2500000", &MicroToUnit), Some(Value::Fixed3(2.5)));
    }

    #[test]
    fn scale_takes_a_count_as_it_is() {
        assert_eq!(scale("1200", &Count), Some(Value::Count(1200)));
        assert_eq!(scale("-1", &Count), None);
    }

    #[test]
    fn scale_refuses_what_is_not_a_whole_number() {
        for raw in ["", "abc", "12.5", "-", "1 2"] {
            for s in [MilliToUnit, MicroToUnit, Count] {
                assert_eq!(scale(raw, &s), None, "{raw:?} as {s:?}");
            }
        }
    }

    #[test]
    fn locate_finds_a_hwmon_by_name() {
        let t = dk::tree();
        assert_eq!(locate(&t.fs(), &HwmonName("ddr_thermal"), "temp1_input").as_deref(), Some("45000"));
    }

    #[test]
    fn locate_takes_the_first_match_in_glob_order() {
        let t = Tree::new();
        t.file("/sys/class/hwmon/hwmon2/name", "emc2305").file("/sys/class/hwmon/hwmon2/pwm1", "2");
        t.file("/sys/class/hwmon/hwmon10/name", "emc2305").file("/sys/class/hwmon/hwmon10/pwm1", "10");
        assert_eq!(locate(&t.fs(), &HwmonName("emc2305"), "pwm1").as_deref(), Some("10"));
    }

    #[test]
    fn locate_skips_a_match_without_the_attribute() {
        let t = Tree::new();
        t.file("/sys/class/hwmon/hwmon0/name", "emc2305");
        t.file("/sys/class/hwmon/hwmon1/name", "emc2305").file("/sys/class/hwmon/hwmon1/pwm1", "76");
        assert_eq!(locate(&t.fs(), &HwmonName("emc2305"), "pwm1").as_deref(), Some("76"));
    }

    #[test]
    fn locate_finds_the_remote_tmp431_channel_by_its_node() {
        let t = dk::tree();
        let found = locate(&t.fs(), &OfSuffix("/i2c@1/temp-sensor@4c"), "temp2_input");
        assert_eq!(found.as_deref(), Some("47063"));
    }

    #[test]
    fn locate_by_node_matches_the_whole_tail() {
        let t = Tree::new();
        let node = "/sys/firmware/devicetree/base/mdio/ethernet-phy@1/child";
        t.dir(node).file("/sys/class/hwmon/hwmon0/temp1_input", "1");
        t.link("/sys/class/hwmon/hwmon0/device/of_node", node);
        assert_eq!(locate(&t.fs(), &OfSuffix("/ethernet-phy@1"), "temp1_input"), None);
    }

    #[test]
    fn locate_by_node_skips_a_hwmon_with_no_node() {
        let t = dk::tree();
        // The thermal zones carry no of_node; the GPY PHY is the only match.
        let found = locate(&t.fs(), &OfSuffix("/ethernet-phy@0"), "temp1_input");
        assert_eq!(found.as_deref(), Some("44237"));
    }

    #[test]
    fn locate_finds_a_hwmon_by_label() {
        let t = dk::tree();
        let found = locate(&t.fs(), &HwmonLabel("USB Power Delivery"), "power1_input");
        assert_eq!(found.as_deref(), Some("12400000"));
    }

    #[test]
    fn locate_finds_an_xfi_cage_by_name() {
        let t = dk::tree();
        assert_eq!(locate(&t.fs(), &Xfi(XfiCage::Xfi0), "temp1_input").as_deref(), Some("47219"));
        assert_eq!(locate(&t.fs(), &Xfi(XfiCage::Xfi1), "temp1_input").as_deref(), Some("46699"));
    }

    #[test]
    fn locate_finds_an_xfi_cage_by_its_paths() {
        let t = Tree::new();
        let mac = "/sys/firmware/devicetree/base/soc/fman@1a00000/ethernet@f0000";
        t.file("/sys/class/hwmon/hwmon0/name", "sfp").file("/sys/class/hwmon/hwmon0/temp1_input", "1");
        t.dir(mac).link("/sys/class/hwmon/hwmon0/device/of_node", mac);
        let other = "/sys/devices/platform/sfp_xfi1";
        t.file("/sys/class/hwmon/hwmon1/name", "sfp").file("/sys/class/hwmon/hwmon1/temp1_input", "2");
        t.dir(other).link("/sys/class/hwmon/hwmon1/device", other);
        assert_eq!(locate(&t.fs(), &Xfi(XfiCage::Xfi0), "temp1_input").as_deref(), Some("1"));
        assert_eq!(locate(&t.fs(), &Xfi(XfiCage::Xfi1), "temp1_input").as_deref(), Some("2"));
    }

    #[test]
    fn locate_with_no_hwmon_is_none() {
        let t = Tree::new();
        assert_eq!(locate(&t.fs(), &HwmonName("ddr_thermal"), "temp1_input"), None);
    }

    #[test]
    fn the_table_has_the_scripts_17_fields() {
        assert_eq!(GATEWAY_DK.len(), 17);
    }
}
