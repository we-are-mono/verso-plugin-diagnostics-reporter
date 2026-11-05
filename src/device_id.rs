// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! The device ID every record carries: the raw eth0 MAC address.

use crate::sysfs::SysFs;
use std::fmt;

const ETH0: &str = "/sys/class/net/eth0/address";

/// A MAC address, "xx:xx:xx:xx:xx:xx" in lowercase hex, so it is safe inside a
/// JSON string.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq)]
pub enum DeviceIdError {
    /// eth0 has no address to read.
    Missing,
    /// The value is not a MAC address.
    NotMac(String),
}

impl fmt::Display for DeviceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceIdError::Missing => write!(f, "cannot read {ETH0}"),
            DeviceIdError::NotMac(value) => write!(f, "device ID {value:?} is not a MAC address"),
        }
    }
}

/// The override when it is not empty, else eth0's address.
pub fn resolve(fs: &dyn SysFs, override_value: &str) -> Result<DeviceId, DeviceIdError> {
    let value = if override_value.is_empty() {
        fs.read(ETH0).unwrap_or_default()
    } else {
        override_value.to_string()
    };
    if value.is_empty() {
        return Err(DeviceIdError::Missing);
    }
    let lower = value.to_ascii_lowercase();
    if is_mac(&lower) {
        Ok(DeviceId(lower))
    } else {
        Err(DeviceIdError::NotMac(value))
    }
}

fn is_mac(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::tree::Tree;

    #[test]
    fn an_override_is_lowercased() {
        let t = Tree::new();
        assert_eq!(resolve(&t.fs(), "AA:BB:CC:DD:EE:FF").unwrap().as_str(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn an_override_that_is_no_mac_is_refused() {
        let t = Tree::new();
        assert_eq!(resolve(&t.fs(), "aa:bb\""), Err(DeviceIdError::NotMac("aa:bb\"".into())));
        assert!(resolve(&t.fs(), "aa:bb:cc:dd:ee:fg").is_err());
        assert!(resolve(&t.fs(), "aa:bb:cc:dd:ee:ff:00").is_err());
    }

    #[test]
    fn with_no_override_eth0_is_read() {
        let t = Tree::new();
        t.file("/sys/class/net/eth0/address", "e8:f6:d7:00:17:df");
        assert_eq!(resolve(&t.fs(), "").unwrap().as_str(), "e8:f6:d7:00:17:df");
    }

    #[test]
    fn a_missing_or_empty_eth0_is_refused() {
        let t = Tree::new();
        assert_eq!(resolve(&t.fs(), ""), Err(DeviceIdError::Missing));
        t.file("/sys/class/net/eth0/address", "");
        assert_eq!(resolve(&t.fs(), ""), Err(DeviceIdError::Missing));
    }
}
