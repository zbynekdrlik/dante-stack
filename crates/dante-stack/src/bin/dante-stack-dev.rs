// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 iemmixer contributors.

//! `dante-stack-dev`: the stack's own device binary, for its tests and the
//! real-hardware gate. Glue only: [`dante_stack::daemon::cli::main`] does
//! the work, as it does for every application's binary.

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    dante_stack::daemon::cli::main(dante_stack::daemon::Build::OWN)
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("dante-stack-dev runs on Linux only");
    std::process::ExitCode::from(2)
}
