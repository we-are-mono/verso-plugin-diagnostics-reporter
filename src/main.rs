// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! verso-plugin-diag: sends the Mono Gateway's sensor readings to
//! mono-collector every 15 minutes, and shows what it last did on
//! System › Mono feedback.
//!
//!   verso-plugin-diag        serve the page on the plugin's socket
//!   verso-plugin-diag push   run the push loop

mod device_id;
mod http;
mod page;
mod push;
mod record;
mod sensors;
mod settings;
mod status;
mod sysfs;

use std::path::PathBuf;
use std::time::SystemTime;
use verso_plugin::{serve, Envelope};

/// The plugin's run directory: the socket, the status file and the body file.
fn run_dir() -> PathBuf {
    std::env::var("VERSO_DIAG_RUN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/run/verso-plugin-diag"))
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        None => serve(
            "diag",
            |r| page::get(r, &run_dir(), SystemTime::now()),
            // The manifest declares no write scope, so the shell posts nothing.
            |_, _| Envelope::not_found(),
        ),
        Some("push") => std::process::exit(push::main(&run_dir())),
        Some(_) => {
            eprintln!("usage: verso-plugin-diag [push]");
            std::process::exit(2);
        }
    }
}
