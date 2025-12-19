//! protontool-launch - Launch Windows executables using Proton.
//!
//! A companion binary that provides a simple way to run .exe files
//! through an existing Steam game's or custom Wine prefix.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use protontool::cli::util::{enable_logging, exit_with_error, ArgParser};
use protontool::gui::{select_steam_installation, select_steam_library_paths};
use protontool::steam::{find_steam_installations, get_steam_apps, get_steam_lib_paths, SteamApp};
use protontool::util::{output_to_string, shell_quote, which};

/// Target environment for launching the executable.
#[derive(Debug)]
enum LaunchTarget {
    SteamApp(u32),
    CustomPrefix(PathBuf),
}

/// Entry point - parse arguments and launch executable via protontool CLI.
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut parser = ArgParser::new(
        "protontool-launch",
        "Utility for launching Windows executables using protontool\n\n\
         Usage:\n\n\
         Launch EXECUTABLE and pick the Steam app using a dialog.\n\
         $ protontool-launch EXECUTABLE [ARGS]\n\n\
         Launch EXECUTABLE for Steam app APPID\n\
         $ protontool-launch --appid APPID EXECUTABLE [ARGS]\n\n\
         Launch EXECUTABLE using a custom prefix\n\
         $ protontool-launch --prefix PREFIX_NAME EXECUTABLE [ARGS]\n\n\
         Environment variables:\n\n\
         PROTON_VERSION: name of the preferred Proton installation\n\
         STEAM_DIR: path to custom Steam installation",
    );

    parser.add_flag(
        "no_term",
        &["--no-term"],
        "Program was launched from desktop",
    );
    parser.add_flag("verbose", &["-v", "--verbose"], "Increase log verbosity");
    parser.add_flag(
        "background_wineserver",
        &["--background-wineserver"],
        "Start wineserver in background before running commands",
    );
    parser.add_option("appid", &["--appid"], "Steam app ID");
    parser.add_option("prefix", &["--prefix"], "Use a custom prefix by name");
    parser.add_flag(
        "cwd_app",
        &["--cwd-app"],
        "Set working directory to app's install dir",
    );
    parser.add_flag("help", &["-h", "--help"], "Show help");

    let parsed = match parser.parse(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", parser.help());
            eprintln!("protontool-launch: error: {}", e);
            process::exit(2);
        }
    };

    if parsed.get_flag("help") {
        println!("{}", parser.help());
        return;
    }

    let no_term = parsed.get_flag("no_term");
    let verbose = parsed.get_count("verbose");

    enable_logging(verbose);

    let positional = parsed.positional();
    if positional.is_empty() {
        eprintln!("{}", parser.help());
        eprintln!("protontool-launch: error: EXECUTABLE is required");
        process::exit(2);
    }

    let executable = &positional[0];
    let exec_args: Vec<String> = if positional.len() > 1 {
        positional[1..].to_vec()
    } else {
        vec![]
    };

    let executable_path = PathBuf::from(executable);
    if !executable_path.exists() {
        exit_with_error(&format!("Executable not found: {}", executable), no_term);
    }

    let steam_installations = find_steam_installations();
    if steam_installations.is_empty() {
        exit_with_error("Steam installation directory could not be found.", no_term);
    }

    let steam_installation = match select_steam_installation(&steam_installations) {
        Some(inst) => inst,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let appid: Option<u32> = parsed.get_option("appid").and_then(|s| s.parse().ok());
    let prefix_name: Option<String> = parsed.get_option("prefix").map(|s| s.to_string());

    // Get prefixes directory
    let prefixes_dir = protontool::config::get_prefixes_dir();

    // Determine launch mode: custom prefix, steam app, or show selection
    let target = if let Some(name) = prefix_name {
        // Use specified custom prefix
        let prefix_path = prefixes_dir.join(&name);
        if !prefix_path.exists() {
            exit_with_error(
                &format!(
                    "Custom prefix '{}' not found at {}",
                    name,
                    prefix_path.display()
                ),
                no_term,
            );
        }
        LaunchTarget::CustomPrefix(prefix_path)
    } else if let Some(id) = appid {
        LaunchTarget::SteamApp(id)
    } else {
        // Show selection dialog for both Steam apps and custom prefixes
        let extra_paths = select_steam_library_paths();
        let steam_lib_paths = get_steam_lib_paths(&steam_installation.steam_path, &extra_paths);
        let steam_apps = get_steam_apps(
            &steam_installation.steam_root,
            &steam_installation.steam_path,
            &steam_lib_paths,
        );
        let windows_apps: Vec<_> = steam_apps
            .into_iter()
            .filter(|app| app.prefix_path.is_some())
            .collect();

        // Try to show combined selection
        match select_launch_target_gui(&windows_apps, &prefixes_dir, &steam_installation.steam_path)
        {
            Some(target) => target,
            None => {
                exit_with_error("No target was selected.", no_term);
            }
        }
    };

    let mut cli_args: Vec<String> = Vec::new();

    if verbose > 0 {
        cli_args.push(format!("-{}", "v".repeat(verbose as usize)));
    }

    if parsed.get_flag("background_wineserver") {
        cli_args.push("--background-wineserver".to_string());
    }

    if no_term {
        cli_args.push("--no-term".to_string());
    }

    if parsed.get_flag("cwd_app") {
        cli_args.push("--cwd-app".to_string());
    }

    let quoted_exec = shell_quote(&executable_path.to_string_lossy());
    let quoted_args: Vec<String> = exec_args.iter().map(|a| shell_quote(a)).collect();

    // Don't include "wine" - the command mode runs through wine already
    let inner_args = format!("{} {}", quoted_exec, quoted_args.join(" "));

    cli_args.push("-c".to_string());
    cli_args.push(inner_args);

    match target {
        LaunchTarget::SteamApp(appid) => {
            cli_args.push(appid.to_string());
        }
        LaunchTarget::CustomPrefix(prefix_path) => {
            cli_args.push("--prefix".to_string());
            cli_args.push(prefix_path.to_string_lossy().to_string());
        }
    }

    protontool::cli::main_cli(Some(cli_args));
}

/// Show GUI dialog to select either a Steam app or custom prefix as launch target.
/// Combines both options in a single list for user selection.
fn select_launch_target_gui(
    steam_apps: &[SteamApp],
    prefixes_dir: &Path,
    _steam_path: &Path,
) -> Option<LaunchTarget> {
    // Find zenity or yad
    let gui_tool = which("zenity").or_else(|| which("yad"))?;

    // Collect custom prefixes
    let custom_prefixes: Vec<_> = std::fs::read_dir(prefixes_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect()
        })
        .unwrap_or_default();

    if steam_apps.is_empty() && custom_prefixes.is_empty() {
        return None;
    }

    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        "Select target to run executable".to_string(),
        "--column".to_string(),
        "Type".to_string(),
        "--column".to_string(),
        "Name".to_string(),
        "--column".to_string(),
        "ID/Path".to_string(),
        "--print-column".to_string(),
        "ALL".to_string(),
        "--width".to_string(),
        "700".to_string(),
        "--height".to_string(),
        "500".to_string(),
    ];

    // Add custom prefixes first
    for entry in &custom_prefixes {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().to_string_lossy().to_string();
        args.push("[Custom]".to_string());
        args.push(name);
        args.push(path);
    }

    // Add Steam apps
    for app in steam_apps {
        args.push("[Steam]".to_string());
        args.push(app.name.clone());
        args.push(app.appid.to_string());
    }

    let output = Command::new(&gui_tool).args(&args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let selected = output_to_string(&output);
    if selected.is_empty() {
        return None;
    }

    // Parse selection: "Type|Name|ID/Path"
    let parts: Vec<&str> = selected.split('|').collect();
    if parts.len() < 3 {
        return None;
    }

    let type_col = parts[0];
    let id_or_path = parts[2];

    if type_col == "[Custom]" {
        Some(LaunchTarget::CustomPrefix(PathBuf::from(id_or_path)))
    } else if type_col == "[Steam]" {
        id_or_path.parse::<u32>().ok().map(LaunchTarget::SteamApp)
    } else {
        None
    }
}
