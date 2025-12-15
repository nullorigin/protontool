pub mod cli;
pub mod config;
pub mod gui;
pub mod wine_data;
pub mod log;
pub mod steam;
pub mod util;
pub mod vdf;
pub mod winetricks;
pub use winetricks::*;
pub use cli::main_cli;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    // Initialize the logging system
    if let Err(e) = log::Logger::init() {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }
    
    main_cli(None);
}
