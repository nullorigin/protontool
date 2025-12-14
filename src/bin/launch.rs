use std::env;
use std::path::PathBuf;
use std::process;

use protontool::cli::util::{ArgParser, enable_logging, exit_with_error};
use protontool::steam::find_steam_installations;
use protontool::gui::select_steam_installation;
use protontool::util::shell_quote;

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
         Environment variables:\n\n\
         PROTON_VERSION: name of the preferred Proton installation\n\
         STEAM_DIR: path to custom Steam installation",
    );

    parser.add_flag("no_term", &["--no-term"], "Program was launched from desktop");
    parser.add_flag("verbose", &["-v", "--verbose"], "Increase log verbosity");
    parser.add_flag("no_runtime", &["--no-runtime"], "Disable Steam Runtime");
    parser.add_flag("no_bwrap", &["--no-bwrap"], "Disable bwrap containerization");
    parser.add_flag("background_wineserver", &["--background-wineserver"], "Launch background wineserver");
    parser.add_flag("no_background_wineserver", &["--no-background-wineserver"], "No background wineserver");
    parser.add_option("appid", &["--appid"], "Steam app ID");
    parser.add_flag("cwd_app", &["--cwd-app"], "Set working directory to app's install dir");
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

    let _steam_installation = match select_steam_installation(&steam_installations) {
        Some(inst) => inst,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let appid: Option<u32> = parsed.get_option("appid").and_then(|s| s.parse().ok());

    let mut cli_args: Vec<String> = Vec::new();

    if verbose > 0 {
        cli_args.push(format!("-{}", "v".repeat(verbose as usize)));
    }

    if parsed.get_flag("no_runtime") {
        cli_args.push("--no-runtime".to_string());
    }

    if parsed.get_flag("no_bwrap") {
        cli_args.push("--no-bwrap".to_string());
    }

    if parsed.get_flag("background_wineserver") {
        cli_args.push("--background-wineserver".to_string());
    } else if parsed.get_flag("no_background_wineserver") {
        cli_args.push("--no-background-wineserver".to_string());
    }

    if no_term {
        cli_args.push("--no-term".to_string());
    }

    if parsed.get_flag("cwd_app") {
        cli_args.push("--cwd-app".to_string());
    }

    let quoted_exec = shell_quote(&executable_path.to_string_lossy());
    let quoted_args: Vec<String> = exec_args.iter().map(|a| shell_quote(a)).collect();

    let inner_args = format!("wine {} {}", quoted_exec, quoted_args.join(" "));

    cli_args.push("-c".to_string());
    cli_args.push(inner_args);

    if let Some(id) = appid {
        cli_args.push(id.to_string());
    } else {
        todo!("GUI app selection not yet implemented for protontool-launch");
    }

    protontool::cli::main_cli(Some(cli_args));
}
