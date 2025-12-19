//! Valve Data Format (VDF) parser module.
//!
//! Parses Steam's VDF files (libraryfolders.vdf, appmanifest_*.acf, config.vdf)
//! into a key-value dictionary structure.

mod parser;
mod vdict;

pub use parser::*;
pub use vdict::*;
