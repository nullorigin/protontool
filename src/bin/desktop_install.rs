//! protontool-desktop-install - Install .desktop entries for protontool.
//!
//! Creates application shortcuts in ~/.local/share/applications/ for:
//! - protontool: Main GUI for managing prefixes
//! - protontool-launch: Quick launcher for .exe files

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};

use protontool::cli::util::ArgParser;

/// Install .desktop files to ~/.local/share/applications/.
/// Returns the installation directory path on success.
fn install_desktop_entries() -> Result<PathBuf, String> {
    let home = env::var("HOME").map_err(|_| "HOME not set")?;
    let applications_dir = PathBuf::from(&home).join(".local/share/applications");

    fs::create_dir_all(&applications_dir)
        .map_err(|e| format!("Failed to create applications dir: {}", e))?;

    let desktop_content = r#"[Desktop Entry]
Type=Application
Name=Protontool
Comment=Manage Wine/Proton prefixes and install Windows components
Exec=protontool --gui --no-term
Icon=wine
Terminal=false
Categories=Utility;Game;
"#;

    let launch_content = r#"[Desktop Entry]
Type=Application
Name=Protontool Launch
Comment=Launch Windows executables using Proton
Exec=protontool-launch --no-term %f
Icon=wine
Terminal=false
Categories=Utility;Game;
MimeType=application/x-ms-dos-executable;application/x-msdos-program;
"#;

    let desktop_path = applications_dir.join("protontool.desktop");
    let launch_path = applications_dir.join("protontool-launch.desktop");

    fs::write(&desktop_path, desktop_content)
        .map_err(|e| format!("Failed to write desktop file: {}", e))?;

    fs::write(&launch_path, launch_content)
        .map_err(|e| format!("Failed to write launch desktop file: {}", e))?;

    let _ = Command::new("update-desktop-database")
        .arg(&applications_dir)
        .status();

    Ok(applications_dir)
}

/// Entry point - parse arguments and install desktop entries.
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut parser = ArgParser::new(
        "protontool-desktop-install",
        "Install protontool application shortcuts for the local user",
    );

    parser.add_flag("help", &["-h", "--help"], "Show help");

    let parsed = match parser.parse(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", parser.help());
            eprintln!("protontool-desktop-install: error: {}", e);
            process::exit(2);
        }
    };

    if parsed.get_flag("help") {
        println!("{}", parser.help());
        return;
    }

    println!("Installing .desktop files for the local user...");

    match install_desktop_entries() {
        Ok(install_dir) => {
            println!(
                "\nDone. Files have been installed under {}",
                install_dir.display()
            );
            println!(
                "The protontool shortcut and protontool-launch desktop entries should now work."
            );
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
