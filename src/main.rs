pub mod cli;
pub mod config;
pub mod gui;
pub mod steam;
pub mod util;
pub mod vdf;
pub mod winetricks;
pub use winetricks::*;
pub use cli::main_cli;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    main_cli(None);
}
