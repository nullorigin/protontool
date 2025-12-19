//! protontool library crate.
//!
//! This lib.rs exists to expose protontool's modules for doc tests and as a library.
//! The main binary entry point is in main.rs which re-exports these modules.

pub mod cli;
pub mod config;
pub mod gui;
pub mod log;
pub mod steam;
pub mod util;
pub mod vdf;
pub mod wine;
pub mod wine_data;

/// Package version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
