pub mod util;

use std::env;
use std::path::PathBuf;
use std::process;

use crate::cli::util::{ArgParser, enable_logging, exit_with_error};
use crate::gui::{prompt_filesystem_access, select_steam_app_with_gui, select_steam_installation, select_steam_library_paths, select_verb_category_gui, select_verbs_with_gui, show_main_menu_gui, GuiAction, select_proton_with_gui, get_prefix_name_gui, select_prefix_location_gui, select_custom_prefix_gui};
use crate::steam::{find_proton_app, find_proton_by_name, find_steam_installations, get_proton_apps, get_steam_apps, get_steam_lib_paths};
use crate::winetricks::Winetricks;

pub fn main_cli(args: Option<Vec<String>>) {
    let args = args.unwrap_or_else(|| env::args().skip(1).collect());
    
    let mut parser = ArgParser::new(
        "protontool",
        "A tool for managing Wine/Proton prefixes with built-in component installation.\n\n\
         Usage:\n\n\
         Install components (DLLs, fonts, settings) for a Steam game:\n\
         $ protontool APPID <verb> [verb...]\n\n\
         Search for games to find the APPID:\n\
         $ protontool -s GAME_NAME\n\n\
         List all installed games:\n\
         $ protontool -l\n\n\
         Launch the GUI to select games and components:\n\
         $ protontool --gui\n\n\
         Create a custom prefix (non-Steam apps):\n\
         $ protontool --create-prefix ~/MyPrefix --proton 'Proton 9.0'\n\n\
         Environment variables:\n\n\
         PROTON_VERSION: name of the preferred Proton installation\n\
         STEAM_DIR: path to custom Steam installation\n\
         WINE: path to a custom 'wine' executable\n\
         WINESERVER: path to a custom 'wineserver' executable",
    );

    parser.add_flag("verbose", &["-v", "--verbose"], "Increase log verbosity");
    parser.add_flag("no_term", &["--no-term"], "Program was launched from desktop");
    parser.add_option("search", &["-s", "--search"], "Search for game(s) with the given name");
    parser.add_flag("list", &["-l", "--list"], "List all apps");
    parser.add_option("command", &["-c", "--command"], "Run a command with Wine environment variables");
    parser.add_flag("gui", &["--gui"], "Launch the protontool GUI");
    parser.add_flag("no_runtime", &["--no-runtime"], "Disable Steam Runtime");
    parser.add_flag("no_bwrap", &["--no-bwrap"], "Disable bwrap containerization");
    parser.add_flag("background_wineserver", &["--background-wineserver"], "Launch background wineserver");
    parser.add_flag("no_background_wineserver", &["--no-background-wineserver"], "No background wineserver");
    parser.add_flag("cwd_app", &["--cwd-app"], "Set working directory to app's install dir");
    parser.add_multi_option("steam_library", &["--steam-library", "-S"], "Additional Steam library path (can be specified multiple times)");
    parser.add_option("create_prefix", &["--create-prefix"], "Create a new Wine prefix at the given path");
    parser.add_option("prefix", &["--prefix", "-p"], "Use an existing custom prefix path");
    parser.add_option("proton", &["--proton"], "Proton version to use (e.g., 'Proton 9.0')");
    parser.add_flag("version", &["-V", "--version"], "Show version");
    parser.add_flag("help", &["-h", "--help"], "Show help");

    let parsed = match parser.parse(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", parser.help());
            eprintln!("protontool: error: {}", e);
            process::exit(2);
        }
    };

    if parsed.get_flag("help") {
        println!("{}", parser.help());
        return;
    }

    if parsed.get_flag("version") {
        println!("protontool ({})", crate::VERSION);
        return;
    }

    let no_term = parsed.get_flag("no_term");
    let verbose = parsed.get_count("verbose");

    enable_logging(verbose);

    let do_command = parsed.get_option("command").is_some();
    let do_list_apps = parsed.get_option("search").is_some() || parsed.get_flag("list");
    let do_gui = parsed.get_flag("gui");
    let do_create_prefix = parsed.get_option("create_prefix").is_some();
    let do_use_prefix = parsed.get_option("prefix").is_some();
    
    let positional = parsed.positional();
    let appid: Option<u32> = positional.first().and_then(|s| s.parse().ok());
    let verbs_to_run: Vec<String> = if positional.len() > 1 {
        positional[1..].to_vec()
    } else {
        vec![]
    };
    let do_run_verbs = appid.is_some() && !verbs_to_run.is_empty();

    if !do_command && !do_list_apps && !do_gui && !do_run_verbs && !do_create_prefix && !do_use_prefix {
        if args.is_empty() {
            // Default to GUI mode when no args
            run_gui_mode(no_term);
            return;
        }
        println!("{}", parser.help());
        return;
    }

    let action_count = [do_list_apps, do_gui, do_run_verbs, do_command, do_create_prefix, do_use_prefix]
        .iter()
        .filter(|&&x| x)
        .count();

    if action_count != 1 {
        eprintln!("Only one action can be performed at a time.");
        println!("{}", parser.help());
        return;
    }

    if do_gui {
        run_gui_mode(no_term);
    } else if do_list_apps {
        run_list_mode(&parsed, no_term);
    } else if do_run_verbs {
        run_verb_mode(appid.unwrap(), &verbs_to_run, &parsed, no_term);
    } else if do_command {
        let cmd = parsed.get_option("command").unwrap();
        run_command_mode(appid, &cmd, &parsed, no_term);
    } else if do_create_prefix {
        let prefix_path = parsed.get_option("create_prefix").unwrap();
        run_create_prefix_mode(&prefix_path, &parsed, no_term);
    } else if do_use_prefix {
        let prefix_path = parsed.get_option("prefix").unwrap();
        run_custom_prefix_mode(&prefix_path, &verbs_to_run, &parsed, no_term);
    }
}

fn get_steam_context(no_term: bool, extra_libraries: &[String]) -> Option<(PathBuf, PathBuf, Vec<PathBuf>)> {
    let steam_installations = find_steam_installations();
    if steam_installations.is_empty() {
        exit_with_error("Steam installation directory could not be found.", no_term);
    }

    let installation = select_steam_installation(&steam_installations)?;
    let steam_path = installation.steam_path.clone();
    let steam_root = installation.steam_root.clone();

    let extra_paths: Vec<PathBuf> = extra_libraries.iter().map(PathBuf::from).collect();
    let steam_lib_paths = get_steam_lib_paths(&steam_path, &extra_paths);

    let paths: Vec<&std::path::Path> = vec![&steam_path, &steam_root];
    prompt_filesystem_access(&paths, no_term);

    Some((steam_path, steam_root, steam_lib_paths))
}

fn run_gui_mode(no_term: bool) {
    // Show main menu to choose action
    loop {
        let action = match show_main_menu_gui() {
            Some(a) => a,
            None => return, // User cancelled
        };

        match action {
            GuiAction::ManageGame => run_gui_manage_game(no_term),
            GuiAction::CreatePrefix => run_gui_create_prefix(no_term),
            GuiAction::ManagePrefix => run_gui_manage_prefix(no_term),
        }
    }
}

fn run_gui_manage_game(no_term: bool) {
    // First, let user add extra Steam library paths via GUI
    let extra_lib_paths = select_steam_library_paths();
    let extra_libs: Vec<String> = extra_lib_paths.iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);

    let windows_apps: Vec<_> = steam_apps.iter()
        .filter(|app| app.is_windows_app())
        .collect();

    if windows_apps.is_empty() {
        exit_with_error(
            "Found no games. You need to launch a game at least once before protontool can find it.",
            no_term
        );
    }

    let steam_app = match select_steam_app_with_gui(&steam_apps, None, &steam_path) {
        Some(app) => app,
        None => return,
    };

    let proton_app = match find_proton_app(&steam_path, &steam_apps, steam_app.appid) {
        Some(app) => app,
        None => {
            exit_with_error("Proton installation could not be found!", no_term);
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error(
            "Proton installation is incomplete. Have you launched a Steam app using this Proton version at least once?",
            no_term
        );
    }

    let prefix_path = steam_app.prefix_path.as_ref().unwrap();
    let verb_runner = Winetricks::new(&proton_app, prefix_path);

    // Show category selection, then verb selection
    loop {
        let category = match select_verb_category_gui() {
            Some(cat) => cat,
            None => return, // User cancelled - go back to main menu
        };

        let verbs = verb_runner.list_verbs(Some(category));
        let selected = select_verbs_with_gui(&verbs, Some(&format!("Select {} to install", category.as_str())));

        if selected.is_empty() {
            continue; // Go back to category selection
        }

        // Run selected verbs
        for verb_name in &selected {
            println!("Running verb: {}", verb_name);
            if let Err(e) = verb_runner.run_verb(verb_name) {
                eprintln!("Error running {}: {}", verb_name, e);
            }
        }

        println!("Completed running verbs.");
    }
}

fn run_gui_create_prefix(no_term: bool) {
    // Get prefix name from user
    let prefix_name = match get_prefix_name_gui() {
        Some(name) => name,
        None => return,
    };

    // Get prefix location
    let prefix_path = match select_prefix_location_gui(&prefix_name) {
        Some(path) => path,
        None => return,
    };

    // Get Steam context for Proton selection
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &[]) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);
    let proton_apps = get_proton_apps(&steam_apps);

    if proton_apps.is_empty() {
        exit_with_error("No Proton installations found. Please install Proton through Steam first.", no_term);
    }

    // Let user select Proton version
    let proton_app = match select_proton_with_gui(&proton_apps) {
        Some(app) => app,
        None => return,
    };

    if !proton_app.is_proton_ready {
        exit_with_error(
            "Selected Proton installation is not ready. Please launch a game with this Proton version first.",
            no_term
        );
    }

    // Create the prefix
    println!("Creating Wine prefix at: {}", prefix_path.display());
    println!("Using Proton: {}", proton_app.name);

    if let Err(e) = std::fs::create_dir_all(&prefix_path) {
        exit_with_error(&format!("Failed to create prefix directory: {}", e), no_term);
    }

    let wine_ctx = crate::winetricks::WineContext::from_proton(&proton_app, &prefix_path);
    
    println!("Initializing prefix with wineboot...");
    match wine_ctx.run_wine(&["wineboot", "--init"]) {
        Ok(output) => {
            if !output.status.success() {
                eprintln!("Warning: wineboot returned non-zero exit code");
            }
        }
        Err(e) => {
            exit_with_error(&format!("Failed to initialize prefix: {}", e), no_term);
        }
    }

    // Save metadata
    let metadata_path = prefix_path.join(".protontool");
    let metadata = format!(
        "proton_name={}\nproton_path={}\ncreated={}\n",
        proton_app.name,
        proton_app.install_path.display(),
        chrono_lite_now()
    );
    std::fs::write(&metadata_path, metadata).ok();

    println!("Prefix '{}' created successfully!", prefix_name);
}

fn run_gui_manage_prefix(no_term: bool) {
    // Get the default prefixes directory
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    let prefixes_dir = PathBuf::from(format!("{}/.local/share/protontool-prefixes", home));

    // Ensure directory exists
    std::fs::create_dir_all(&prefixes_dir).ok();

    // Let user select a prefix
    let prefix_path = match select_custom_prefix_gui(&prefixes_dir) {
        Some(path) => path,
        None => return,
    };

    // Get Steam context for Proton
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &[]) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);

    // Try to read saved Proton info
    let metadata_path = prefix_path.join(".protontool");
    let proton_app = if let Ok(metadata) = std::fs::read_to_string(&metadata_path) {
        let proton_name = metadata.lines()
            .find(|l| l.starts_with("proton_name="))
            .and_then(|l| l.strip_prefix("proton_name="));
        
        if let Some(name) = proton_name {
            find_proton_by_name(&steam_apps, name)
        } else {
            None
        }
    } else {
        None
    };

    let proton_app = match proton_app {
        Some(app) => {
            println!("Using saved Proton version: {}", app.name);
            app
        }
        None => {
            let proton_apps = get_proton_apps(&steam_apps);
            match select_proton_with_gui(&proton_apps) {
                Some(app) => app,
                None => return,
            }
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error("Proton installation is not ready.", no_term);
    }

    let verb_runner = Winetricks::new(&proton_app, &prefix_path);

    // Interactive verb selection
    loop {
        let category = match select_verb_category_gui() {
            Some(cat) => cat,
            None => return,
        };

        let verb_list = verb_runner.list_verbs(Some(category));
        let selected = select_verbs_with_gui(&verb_list, Some(&format!("Select {} to install", category.as_str())));

        if selected.is_empty() {
            continue;
        }

        for verb_name in &selected {
            println!("Running verb: {}", verb_name);
            if let Err(e) = verb_runner.run_verb(verb_name) {
                eprintln!("Error running {}: {}", verb_name, e);
            }
        }

        println!("Completed running verbs.");
    }
}

fn run_list_mode(parsed: &util::ParsedArgs, no_term: bool) {
    let extra_libs = parsed.get_multi_option("steam_library").to_vec();
    let verbose = parsed.get_count("verbose") > 0;
    
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    if verbose {
        println!("Steam path: {}", steam_path.display());
        println!("Steam root: {}", steam_root.display());
        println!("Library paths searched:");
        for lib in &steam_lib_paths {
            println!("  - {}", lib.display());
        }
        println!();
    }

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);

    if verbose {
        println!("Total apps found: {}", steam_apps.len());
        println!("Apps with Proton prefix (Windows apps): {}", steam_apps.iter().filter(|a| a.is_windows_app()).count());
        println!("Proton installations: {}", steam_apps.iter().filter(|a| a.is_proton).count());
        println!();
        
        if steam_apps.iter().filter(|a| a.is_windows_app()).count() == 0 {
            println!("No Windows apps found. Showing all detected apps:");
            for app in &steam_apps {
                println!("  {} ({}) - proton: {}, has_prefix: {}", 
                    app.name, app.appid, app.is_proton, app.prefix_path.is_some());
            }
            println!();
        }
    }

    let matching_apps: Vec<_> = if parsed.get_flag("list") {
        steam_apps.iter().filter(|app| app.is_windows_app()).collect()
    } else if let Some(search) = parsed.get_option("search") {
        steam_apps.iter()
            .filter(|app| app.is_windows_app() && app.name_contains(search))
            .collect()
    } else {
        vec![]
    };

    if !matching_apps.is_empty() {
        println!("Found the following games:");
        for app in &matching_apps {
            println!("{} ({})", app.name, app.appid);
        }
        println!("\nTo run protontool for the chosen game, run:");
        println!("$ protontool APPID COMMAND");
    } else {
        println!("Found no games.");
    }

    println!("\nNOTE: A game must be launched at least once before protontool can find the game.");
}

fn run_verb_mode(appid: u32, verbs: &[String], parsed: &util::ParsedArgs, no_term: bool) {
    let extra_libs = parsed.get_multi_option("steam_library").to_vec();
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);

    let steam_app = match steam_apps.iter().find(|app| app.appid == appid && app.is_windows_app()) {
        Some(app) => app.clone(),
        None => {
            exit_with_error(
                "Steam app with the given app ID could not be found. Is it installed and have you launched it at least once?",
                no_term
            );
        }
    };

    let proton_app = match find_proton_app(&steam_path, &steam_apps, appid) {
        Some(app) => app,
        None => {
            exit_with_error("Proton installation could not be found!", no_term);
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error(
            "Proton installation is incomplete. Have you launched a Steam app using this Proton version at least once?",
            no_term
        );
    }

    let prefix_path = steam_app.prefix_path.as_ref().unwrap();
    let verb_runner = Winetricks::new(&proton_app, prefix_path);

    // Run each specified verb
    let mut success = true;
    for verb_name in verbs {
        // Skip if it looks like a flag (starts with -)
        if verb_name.starts_with('-') {
            continue;
        }
        
        println!("Running verb: {}", verb_name);
        match verb_runner.run_verb(verb_name) {
            Ok(()) => println!("Successfully completed: {}", verb_name),
            Err(e) => {
                eprintln!("Error running {}: {}", verb_name, e);
                success = false;
            }
        }
    }

    if success {
        process::exit(0);
    } else {
        process::exit(1);
    }
}

fn run_command_mode(appid: Option<u32>, command: &str, parsed: &util::ParsedArgs, no_term: bool) {
    let extra_libs = parsed.get_multi_option("steam_library").to_vec();
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);

    let appid = match appid {
        Some(id) => id,
        None => {
            exit_with_error("APPID is required for -c/--command mode", no_term);
        }
    };

    let steam_app = match steam_apps.iter().find(|app| app.appid == appid && app.is_windows_app()) {
        Some(app) => app.clone(),
        None => {
            exit_with_error(
                "Steam app with the given app ID could not be found.",
                no_term
            );
        }
    };

    let proton_app = match find_proton_app(&steam_path, &steam_apps, appid) {
        Some(app) => app,
        None => {
            exit_with_error("Proton installation could not be found!", no_term);
        }
    };

    // Use built-in wine context to run the command
    let prefix_path = steam_app.prefix_path.as_ref().unwrap();
    let wine_ctx = crate::winetricks::WineContext::from_proton(&proton_app, prefix_path);

    let cwd_app = parsed.get_flag("cwd_app");
    let _cwd = if cwd_app { Some(steam_app.install_path.to_string_lossy().to_string()) } else { None };

    // Run the command with wine
    match wine_ctx.run_wine(&[command]) {
        Ok(output) => {
            if !output.stdout.is_empty() {
                println!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            process::exit(output.status.code().unwrap_or(0));
        }
        Err(e) => {
            exit_with_error(&format!("Failed to run command: {}", e), no_term);
        }
    }
}

fn run_create_prefix_mode(prefix_path: &str, parsed: &util::ParsedArgs, no_term: bool) {
    let extra_libs = parsed.get_multi_option("steam_library").to_vec();
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);
    let proton_apps = get_proton_apps(&steam_apps);

    if proton_apps.is_empty() {
        exit_with_error("No Proton installations found. Please install Proton through Steam first.", no_term);
    }

    // Find Proton version - either from --proton flag or let user select
    let proton_app = if let Some(proton_name) = parsed.get_option("proton") {
        match find_proton_by_name(&steam_apps, proton_name) {
            Some(app) => app,
            None => {
                eprintln!("Available Proton versions:");
                for app in &proton_apps {
                    eprintln!("  - {}", app.name);
                }
                exit_with_error(&format!("Proton version '{}' not found.", proton_name), no_term);
            }
        }
    } else {
        match select_proton_with_gui(&proton_apps) {
            Some(app) => app,
            None => {
                exit_with_error("No Proton version selected.", no_term);
            }
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error(
            "Selected Proton installation is not ready. Please launch a game with this Proton version first to initialize it.",
            no_term
        );
    }

    let prefix_path = PathBuf::from(prefix_path);
    
    // Create the prefix directory structure
    println!("Creating Wine prefix at: {}", prefix_path.display());
    println!("Using Proton: {}", proton_app.name);

    if let Err(e) = std::fs::create_dir_all(&prefix_path) {
        exit_with_error(&format!("Failed to create prefix directory: {}", e), no_term);
    }

    // Initialize the prefix with Proton's wine
    let wine_ctx = crate::winetricks::WineContext::from_proton(&proton_app, &prefix_path);
    
    // Run wineboot to initialize the prefix
    println!("Initializing prefix with wineboot...");
    match wine_ctx.run_wine(&["wineboot", "--init"]) {
        Ok(output) => {
            if !output.status.success() {
                eprintln!("Warning: wineboot returned non-zero exit code");
                if !output.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                }
            }
        }
        Err(e) => {
            exit_with_error(&format!("Failed to initialize prefix: {}", e), no_term);
        }
    }

    // Save prefix metadata for future use
    let metadata_path = prefix_path.join(".protontool");
    let metadata = format!(
        "proton_name={}\nproton_path={}\ncreated={}\n",
        proton_app.name,
        proton_app.install_path.display(),
        chrono_lite_now()
    );
    std::fs::write(&metadata_path, metadata).ok();

    println!("\nPrefix created successfully!");
    println!("\nTo use this prefix:");
    println!("  protontool --prefix '{}' <verbs>", prefix_path.display());
    println!("  protontool --prefix '{}' -c <command>", prefix_path.display());
}

fn run_custom_prefix_mode(prefix_path: &str, verbs: &[String], parsed: &util::ParsedArgs, no_term: bool) {
    let prefix_path = PathBuf::from(prefix_path);
    
    if !prefix_path.exists() {
        exit_with_error(&format!("Prefix path does not exist: {}", prefix_path.display()), no_term);
    }

    let extra_libs = parsed.get_multi_option("steam_library").to_vec();
    let (steam_path, steam_root, steam_lib_paths) = match get_steam_context(no_term, &extra_libs) {
        Some(ctx) => ctx,
        None => {
            exit_with_error("No Steam installation was selected.", no_term);
        }
    };

    let steam_apps = get_steam_apps(&steam_root, &steam_path, &steam_lib_paths);
    let proton_apps = get_proton_apps(&steam_apps);

    // Try to read saved Proton info from prefix metadata
    let metadata_path = prefix_path.join(".protontool");
    let proton_app = if let Ok(metadata) = std::fs::read_to_string(&metadata_path) {
        let proton_name = metadata.lines()
            .find(|l| l.starts_with("proton_name="))
            .and_then(|l| l.strip_prefix("proton_name="));
        
        if let Some(name) = proton_name {
            find_proton_by_name(&steam_apps, name)
        } else {
            None
        }
    } else {
        None
    };

    // If no saved Proton or --proton flag specified, select one
    let proton_app = if let Some(proton_name) = parsed.get_option("proton") {
        match find_proton_by_name(&steam_apps, proton_name) {
            Some(app) => app,
            None => {
                exit_with_error(&format!("Proton version '{}' not found.", proton_name), no_term);
            }
        }
    } else if let Some(app) = proton_app {
        println!("Using saved Proton version: {}", app.name);
        app
    } else {
        match select_proton_with_gui(&proton_apps) {
            Some(app) => app,
            None => {
                exit_with_error("No Proton version selected.", no_term);
            }
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error("Proton installation is not ready.", no_term);
    }

    let verb_runner = Winetricks::new(&proton_app, &prefix_path);

    if verbs.is_empty() {
        // Interactive mode - show verb selection
        loop {
            let category = match select_verb_category_gui() {
                Some(cat) => cat,
                None => return,
            };

            let verb_list = verb_runner.list_verbs(Some(category));
            let selected = select_verbs_with_gui(&verb_list, Some(&format!("Select {} to install", category.as_str())));

            if selected.is_empty() {
                continue;
            }

            for verb_name in &selected {
                println!("Running verb: {}", verb_name);
                if let Err(e) = verb_runner.run_verb(verb_name) {
                    eprintln!("Error running {}: {}", verb_name, e);
                }
            }

            println!("Completed running verbs.");
        }
    } else {
        // Run specified verbs
        for verb_name in verbs {
            if verb_name.starts_with('-') {
                continue;
            }
            println!("Running verb: {}", verb_name);
            match verb_runner.run_verb(verb_name) {
                Ok(()) => println!("Successfully completed: {}", verb_name),
                Err(e) => eprintln!("Error running {}: {}", verb_name, e),
            }
        }
    }
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}", duration.as_secs())
}
