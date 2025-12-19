//! protontool - A tool for managing Wine/Proton prefixes with built-in component installation.
//!
//! Provides both CLI and GUI interfaces for:
//! - Installing DLLs, fonts, and settings into Wine prefixes
//! - Creating and managing custom prefixes
//! - Running applications with proper Wine/Proton environment

pub mod cli;
pub mod config;
pub mod gui;
pub mod log;
pub mod steam;
pub mod util;
pub mod vdf;
pub mod wine;
pub mod wine_data;
pub use cli::main_cli;
pub use wine::*;

/// Package version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Entry point - initializes logging and runs the CLI.
fn main() {
    // Initialize the logging system
    if let Err(e) = log::Logger::init() {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }

    main_cli(None);
}
