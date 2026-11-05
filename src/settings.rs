// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! What the push loop is told by its init script, through its environment.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct PushSettings {
    /// The collector's endpoint, https only.
    pub url: String,
    pub token: String,
    /// A CA file to trust in place of ca-bundle's; empty for ca-bundle.
    pub ca: String,
    /// A device ID to send in place of eth0's MAC; empty for the MAC.
    pub device_override: String,
}

#[derive(Debug, PartialEq)]
pub enum SettingsError {
    Missing(&'static str),
    Invalid(&'static str),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Missing(name) => write!(f, "{name} is not set"),
            SettingsError::Invalid(name) => write!(f, "{name} is not valid"),
        }
    }
}

pub const URL: &str = "VERSO_DIAG_URL";
pub const TOKEN: &str = "VERSO_DIAG_TOKEN";
pub const CA: &str = "VERSO_DIAG_CA";
pub const DEVICE_ID: &str = "VERSO_DIAG_DEVICE_ID";

/// Reads the VERSO_DIAG_* variables through `env`. The first one missing or
/// not valid is the error.
pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<PushSettings, SettingsError> {
    let get = |name| env(name).unwrap_or_default();
    let (url, token, ca) = (get(URL), get(TOKEN), get(CA));
    if url.is_empty() {
        return Err(SettingsError::Missing(URL));
    }
    if !valid_url(&url) {
        return Err(SettingsError::Invalid(URL));
    }
    if token.is_empty() {
        return Err(SettingsError::Missing(TOKEN));
    }
    if !valid_token(&token) {
        return Err(SettingsError::Invalid(TOKEN));
    }
    if !ca.is_empty() && !valid_path(&ca) {
        return Err(SettingsError::Invalid(CA));
    }
    Ok(PushSettings { url, token, ca, device_override: get(DEVICE_ID) })
}

/// An https URL of characters that need no quoting in a curl config line, a
/// UCI value or a sed replacement: no quote, backslash, space or '|'.
pub fn valid_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    !rest.is_empty()
        && url.len() <= 2048
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~:/?#[]@!&()*+,;=%".contains(&b))
}

/// A token of base64 and URL-safe characters.
pub fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 256
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~+/=-".contains(&b))
}

/// An absolute path of plain characters.
fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 256
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn complete_settings_are_read() {
        let s = from_env(&env(&[
            (URL, "https://diag.example.com/d"),
            (TOKEN, "SuperSecretKey"),
            (DEVICE_ID, "aa:bb:cc:dd:ee:ff"),
        ]))
        .unwrap();
        assert_eq!(s.url, "https://diag.example.com/d");
        assert_eq!(s.token, "SuperSecretKey");
        assert_eq!(s.ca, "");
        assert_eq!(s.device_override, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn a_missing_token_is_named() {
        let got = from_env(&env(&[(URL, "https://diag.example.com/d")]));
        assert_eq!(got, Err(SettingsError::Missing(TOKEN)));
    }

    #[test]
    fn a_missing_url_is_named() {
        assert_eq!(from_env(&env(&[(TOKEN, "x")])), Err(SettingsError::Missing(URL)));
    }

    #[test]
    fn plain_http_is_refused() {
        let got = from_env(&env(&[(URL, "http://x/d"), (TOKEN, "x")]));
        assert_eq!(got, Err(SettingsError::Invalid(URL)));
    }

    #[test]
    fn a_url_that_would_break_a_config_line_is_refused() {
        for url in ["https://", "https://x/\"d", "https://x/ d", "https://x/\\d", "https://x/'d", "https://x|d"] {
            assert!(!valid_url(url), "{url}");
        }
        assert!(valid_url("https://192.168.99.201:8888/d"));
    }

    #[test]
    fn a_token_with_a_space_or_quote_is_refused() {
        for token in ["a b", "a\"b", "a\\b", "a'b"] {
            let got = from_env(&env(&[(URL, "https://x/d"), (TOKEN, token)]));
            assert_eq!(got, Err(SettingsError::Invalid(TOKEN)), "{token}");
        }
    }

    #[test]
    fn a_ca_must_be_an_absolute_plain_path() {
        let got = from_env(&env(&[(URL, "https://x/d"), (TOKEN, "x"), (CA, "ca.pem")]));
        assert_eq!(got, Err(SettingsError::Invalid(CA)));
        let ok = from_env(&env(&[(URL, "https://x/d"), (TOKEN, "x"), (CA, "/etc/verso-plugin-diag/ca.pem")]));
        assert_eq!(ok.unwrap().ca, "/etc/verso-plugin-diag/ca.pem");
    }
}
