pub mod util;

use std::env;
use std::path::PathBuf;
use std::process;

use crate::cli::util::{ArgParser, enable_logging, exit_with_error};
use crate::gui::{prompt_filesystem_access, select_steam_app_with_gui, select_steam_installation, select_steam_library_paths, select_verb_category_gui, select_verbs_with_gui, show_main_menu_gui, GuiAction, select_proton_with_gui, get_prefix_name_gui, select_prefix_location_gui, select_custom_prefix_gui};
use crate::steam::{find_proton_app, find_proton_by_name, find_steam_installations, get_proton_apps, get_steam_apps, get_steam_lib_paths};
use crate::wine::Wine;

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
         Delete a custom prefix:\n\
         $ protontool --delete-prefix ~/MyPrefix\n\n\
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
    parser.add_option("delete_prefix", &["--delete-prefix"], "Delete an existing custom prefix at the given path");
    parser.add_option("prefix", &["--prefix", "-p"], "Use an existing custom prefix path");
    parser.add_option("proton", &["--proton"], "Proton version to use (e.g., 'Proton 9.0')");
    parser.add_option("arch", &["--arch"], "Prefix architecture: win32 or win64 (default: win64)");
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
    let do_delete_prefix = parsed.get_option("delete_prefix").is_some();
    let do_use_prefix = parsed.get_option("prefix").is_some();
    
    let positional = parsed.positional();
    let appid: Option<u32> = positional.first().and_then(|s| s.parse().ok());
    let verbs_to_run: Vec<String> = if positional.len() > 1 {
        positional[1..].to_vec()
    } else {
        vec![]
    };
    let do_run_verbs = appid.is_some() && !verbs_to_run.is_empty();

    if !do_command && !do_list_apps && !do_gui && !do_run_verbs && !do_create_prefix && !do_delete_prefix && !do_use_prefix {
        if args.is_empty() {
            // Default to GUI mode when no args
            run_gui_mode(no_term);
            return;
        }
        println!("{}", parser.help());
        return;
    }

    // Allow combining -c with --prefix (command mode with custom prefix)
    let do_prefix_command = do_command && do_use_prefix;
    
    let action_count = if do_prefix_command {
        1 // Treat prefix + command as single action
    } else {
        [do_list_apps, do_gui, do_run_verbs, do_command, do_create_prefix, do_delete_prefix, do_use_prefix]
            .iter()
            .filter(|&&x| x)
            .count()
    };

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
    } else if do_prefix_command {
        let cmd = parsed.get_option("command").unwrap();
        let prefix_path = parsed.get_option("prefix").unwrap();
        run_prefix_command_mode(&prefix_path, &cmd, &parsed, no_term);
    } else if do_command {
        let cmd = parsed.get_option("command").unwrap();
        run_command_mode(appid, &cmd, &parsed, no_term);
    } else if do_create_prefix {
        let prefix_path = parsed.get_option("create_prefix").unwrap();
        run_create_prefix_mode(&prefix_path, &parsed, no_term);
    } else if do_delete_prefix {
        let prefix_path = parsed.get_option("delete_prefix").unwrap();
        run_delete_prefix_mode(&prefix_path, no_term);
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
            GuiAction::DeletePrefix => run_gui_delete_prefix(no_term),
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
    let verb_runner = Wine::new(&proton_app, prefix_path);

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

    // Let user select architecture
    let arch = match select_arch_gui() {
        Some(a) => a,
        None => return,
    };

    // Create the prefix
    println!("Creating Wine prefix at: {}", prefix_path.display());
    println!("Using Proton: {}", proton_app.name);
    println!("Architecture: {}", arch.as_str());

    if let Err(e) = std::fs::create_dir_all(&prefix_path) {
        exit_with_error(&format!("Failed to create prefix directory: {}", e), no_term);
    }

    let wine_ctx = crate::wine::WineContext::from_proton_with_arch(&proton_app, &prefix_path, arch);
    
    println!("Initializing prefix with wineboot...");
    match wine_ctx.run_wine_no_cwd(&["wineboot", "--init"]) {
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
        "proton_name={}\nproton_path={}\narch={}\ncreated={}\n",
        proton_app.name,
        proton_app.install_path.display(),
        arch.as_str(),
        chrono_lite_now()
    );
    std::fs::write(&metadata_path, metadata).ok();

    println!("Prefix '{}' created successfully!", prefix_name);
}

fn run_gui_delete_prefix(no_term: bool) {
    let prefixes_dir = crate::config::get_prefixes_dir();
    
    // Ensure directory exists
    std::fs::create_dir_all(&prefixes_dir).ok();
    
    // Let user select a prefix to delete
    let prefix_path = match select_custom_prefix_gui(&prefixes_dir) {
        Some(path) => path,
        None => return,
    };
    
    let prefix_name = prefix_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown");
    
    // Confirm deletion
    let gui_tool = match crate::gui::get_gui_tool() {
        Some(tool) => tool,
        None => {
            exit_with_error("No GUI tool available", no_term);
        }
    };
    
    let confirm_text = format!(
        "Are you sure you want to delete the prefix '{}'?\n\nThis will permanently remove:\n{}\n\nThis action cannot be undone!",
        prefix_name,
        prefix_path.display()
    );
    
    let confirm = std::process::Command::new(&gui_tool)
        .args([
            "--question",
            "--title", "Confirm Delete",
            "--text", &confirm_text,
            "--width", "450",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    
    if !confirm {
        println!("Deletion cancelled.");
        return;
    }
    
    // Delete the prefix directory
    match std::fs::remove_dir_all(&prefix_path) {
        Ok(()) => {
            println!("Prefix '{}' deleted successfully.", prefix_name);
            
            // Show success message
            let _ = std::process::Command::new(&gui_tool)
                .args([
                    "--info",
                    "--title", "Prefix Deleted",
                    "--text", &format!("Prefix '{}' has been deleted.", prefix_name),
                    "--width", "300",
                ])
                .status();
        }
        Err(e) => {
            let error_msg = format!("Failed to delete prefix: {}", e);
            eprintln!("{}", error_msg);
            
            let _ = std::process::Command::new(&gui_tool)
                .args([
                    "--error",
                    "--title", "Delete Failed",
                    "--text", &error_msg,
                    "--width", "400",
                ])
                .status();
        }
    }
}

fn run_gui_manage_prefix(no_term: bool) {
    // Get the default prefixes directory
    let prefixes_dir = crate::config::get_prefixes_dir();

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

    // Try to read saved Proton and arch info
    let metadata_path = prefix_path.join(".protontool");
    let metadata_content = std::fs::read_to_string(&metadata_path).ok();
    
    let proton_app = if let Some(ref metadata) = metadata_content {
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
    
    // Read saved architecture (default to win64)
    let saved_arch = metadata_content.as_ref()
        .and_then(|m| m.lines().find(|l| l.starts_with("arch=")))
        .and_then(|l| l.strip_prefix("arch="))
        .and_then(crate::wine::WineArch::from_str)
        .unwrap_or(crate::wine::WineArch::Win64);

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

    let verb_runner = Wine::new_with_arch(&proton_app, &prefix_path, saved_arch);
    let wine_ctx = crate::wine::WineContext::from_proton_with_arch(&proton_app, &prefix_path, saved_arch);

    // Interactive action selection
    loop {
        // Show action menu
        match select_prefix_action_gui() {
            Some(PrefixAction::RunApplication) => {
                if let Some(exe_path) = select_executable_gui() {
                    println!("Running: {}", exe_path.display());
                    // run_wine automatically changes to executable's directory
                    match wine_ctx.run_wine(&[&exe_path.to_string_lossy()]) {
                        Ok(_) => {}
                        Err(e) => eprintln!("Error running application: {}", e),
                    }
                }
            }
            Some(PrefixAction::InstallComponents) => {
                let category = match select_verb_category_gui() {
                    Some(cat) => cat,
                    None => continue,
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
            Some(PrefixAction::WineTools) => {
                if let Some(tool) = select_wine_tool_gui() {
                    println!("Launching: {}", tool);
                    match wine_ctx.run_wine_no_cwd(&[&tool]) {
                        Ok(_) => {}
                        Err(e) => eprintln!("Error launching {}: {}", tool, e),
                    }
                }
            }
            Some(PrefixAction::Settings) => {
                if let Some(setting) = select_prefix_setting_gui() {
                    match setting {
                        PrefixSetting::Dpi => {
                            if let Some(dpi) = select_dpi_gui() {
                                println!("Setting DPI to: {}", dpi);
                                set_wine_dpi(&wine_ctx, dpi);
                            }
                        }
                    }
                }
            }
            Some(PrefixAction::CreateVerb) => {
                run_verb_creator_gui();
            }
            None => return,
        }
    }
}

enum PrefixAction {
    RunApplication,
    InstallComponents,
    WineTools,
    Settings,
    CreateVerb,
}

fn select_prefix_action_gui() -> Option<PrefixAction> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "Select action",
        "--column", "Action",
        "--column", "Description",
        "--print-column", "1",
        "--width", "500",
        "--height", "350",
        "run", "Run an application",
        "install", "Install components (DLLs, fonts, etc.)",
        "tools", "Wine tools (winecfg, regedit, etc.)",
        "settings", "Prefix settings (DPI, etc.)",
        "verb", "Create custom verb",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    match selected.as_str() {
        "run" => Some(PrefixAction::RunApplication),
        "install" => Some(PrefixAction::InstallComponents),
        "tools" => Some(PrefixAction::WineTools),
        "settings" => Some(PrefixAction::Settings),
        "verb" => Some(PrefixAction::CreateVerb),
        _ => None,
    }
}

fn select_executable_gui() -> Option<PathBuf> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    let args = vec![
        "--file-selection",
        "--title", "Select executable to run",
        "--file-filter", "Windows Executables | *.exe *.msi *.bat",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn select_arch_gui() -> Option<crate::wine::WineArch> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "Select prefix architecture",
        "--column", "Architecture",
        "--column", "Description",
        "--print-column", "1",
        "--width", "500",
        "--height", "250",
        "win64", "64-bit Windows (recommended for modern apps)",
        "win32", "32-bit Windows (for legacy apps)",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    crate::wine::WineArch::from_str(&selected)
}

fn select_wine_tool_gui() -> Option<String> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "Select Wine tool",
        "--column", "Tool",
        "--column", "Description",
        "--print-column", "1",
        "--width", "500",
        "--height", "350",
        "winecfg", "Wine configuration",
        "regedit", "Registry editor",
        "taskmgr", "Task manager",
        "explorer", "File explorer",
        "control", "Control panel",
        "cmd", "Command prompt",
        "uninstaller", "Wine uninstaller",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

enum PrefixSetting {
    Dpi,
}

fn select_prefix_setting_gui() -> Option<PrefixSetting> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "Select setting",
        "--column", "Setting",
        "--column", "Description",
        "--print-column", "1",
        "--width", "500",
        "--height", "250",
        "dpi", "Display DPI (scaling)",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match selected.as_str() {
        "dpi" => Some(PrefixSetting::Dpi),
        _ => None,
    }
}

fn select_dpi_gui() -> Option<u32> {
    let gui_tool = crate::gui::get_gui_tool()?;
    
    // DPI options in increments of 48, starting at 96
    let args = vec![
        "--list",
        "--title", "Select DPI",
        "--column", "DPI",
        "--column", "Scale",
        "--print-column", "1",
        "--width", "400",
        "--height", "400",
        "96", "100% (default)",
        "144", "150%",
        "192", "200%",
        "240", "250%",
        "288", "300%",
        "336", "350%",
        "384", "400%",
    ];
    
    let output = std::process::Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    selected.parse().ok()
}

fn set_wine_dpi(wine_ctx: &crate::wine::WineContext, dpi: u32) {
    // Set DPI via registry
    let reg_content = format!(
        "Windows Registry Editor Version 5.00\n\n\
         [HKEY_CURRENT_USER\\Control Panel\\Desktop]\n\
         \"LogPixels\"=dword:{:08x}\n\n\
         [HKEY_CURRENT_USER\\Software\\Wine\\Fonts]\n\
         \"LogPixels\"=dword:{:08x}\n",
        dpi, dpi
    );
    
    // Write to a temp .reg file
    let tmp_dir = std::env::temp_dir();
    let reg_file = tmp_dir.join("protontool_dpi.reg");
    
    if let Err(e) = std::fs::write(&reg_file, &reg_content) {
        eprintln!("Failed to write registry file: {}", e);
        return;
    }
    
    // Import the registry file
    match wine_ctx.run_wine_no_cwd(&["regedit", "/S", &reg_file.to_string_lossy()]) {
        Ok(_) => println!("DPI set to {}. You may need to restart applications for changes to take effect.", dpi),
        Err(e) => eprintln!("Failed to set DPI: {}", e),
    }
    
    // Clean up
    std::fs::remove_file(&reg_file).ok();
}

// ============================================================================
// CUSTOM VERB CREATOR GUI
// ============================================================================

struct VerbData {
    name: String,
    title: String,
    publisher: String,
    year: String,
    category: String,
    action_type: String,
    installer_path: String,
    installer_args: String,
}

impl Default for VerbData {
    fn default() -> Self {
        Self {
            name: String::new(),
            title: String::new(),
            publisher: String::new(),
            year: chrono_lite_now().split('-').next().unwrap_or("2024").to_string(),
            category: "app".to_string(),
            action_type: "local_installer".to_string(),
            installer_path: String::new(),
            installer_args: "/S".to_string(),
        }
    }
}

impl VerbData {
    fn derive_name_from_title(&mut self) {
        self.name = self.title
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .replace(' ', "");
    }
    
    fn to_toml(&self) -> String {
        let args_array = self.installer_args
            .split_whitespace()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ");
        
        format!(
            r#"[verb]
name = "{}"
category = "{}"
title = "{}"
publisher = "{}"
year = "{}"

[[actions]]
type = "{}"
path = "{}"
args = [{}]
"#,
            self.name,
            self.category,
            self.title,
            self.publisher,
            self.year,
            self.action_type,
            self.installer_path,
            args_array
        )
    }
    
    fn from_toml(content: &str) -> Option<Self> {
        let mut data = Self::default();
        
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');
                
                match key {
                    "name" => data.name = value.to_string(),
                    "title" => data.title = value.to_string(),
                    "publisher" => data.publisher = value.to_string(),
                    "year" => data.year = value.to_string(),
                    "category" => data.category = value.to_string(),
                    "type" => data.action_type = value.to_string(),
                    "path" => data.installer_path = value.to_string(),
                    "args" => {
                        // Parse array like ["/S", "/D=path"]
                        let inner = value.trim_start_matches('[').trim_end_matches(']');
                        data.installer_args = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"'))
                            .collect::<Vec<_>>()
                            .join(" ");
                    }
                    _ => {}
                }
            }
        }
        
        if data.name.is_empty() && data.title.is_empty() {
            None
        } else {
            Some(data)
        }
    }
}

fn run_verb_creator_gui() {
    let gui_tool = match crate::gui::get_gui_tool() {
        Some(tool) => tool,
        None => {
            eprintln!("No GUI tool available");
            return;
        }
    };
    
    // Initial dialog: Import existing or create new?
    let output = std::process::Command::new(&gui_tool)
        .args([
            "--list",
            "--title", "Custom Verb Creator",
            "--column", "Option",
            "--column", "Description",
            "--print-column", "1",
            "--width", "500",
            "--height", "250",
            "new", "Create a new custom verb",
            "import", "Import existing TOML file",
        ])
        .output();
    
    let mut verb_data = VerbData::default();
    
    if let Ok(out) = output {
        if out.status.success() {
            let choice = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if choice == "import" {
                if let Some(data) = import_verb_toml_gui(&gui_tool) {
                    verb_data = data;
                } else {
                    return;
                }
            }
        } else {
            return;
        }
    } else {
        return;
    }
    
    // Show advanced options checkbox
    let show_advanced = std::process::Command::new(&gui_tool)
        .args([
            "--question",
            "--title", "Verb Creator Mode",
            "--text", "Show advanced options?\n\nSimple mode derives some values automatically.\nAdvanced mode gives full control over all fields.",
            "--ok-label", "Advanced",
            "--cancel-label", "Simple",
            "--width", "400",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    
    // Run the appropriate editor
    let result = if show_advanced {
        edit_verb_advanced_gui(&gui_tool, &mut verb_data)
    } else {
        edit_verb_simple_gui(&gui_tool, &mut verb_data)
    };
    
    if !result {
        return;
    }
    
    // Save dialog
    save_verb_gui(&gui_tool, &verb_data);
}

fn import_verb_toml_gui(gui_tool: &std::path::Path) -> Option<VerbData> {
    let output = std::process::Command::new(gui_tool)
        .args([
            "--file-selection",
            "--title", "Import TOML verb file",
            "--file-filter", "TOML files | *.toml",
        ])
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    
    let content = std::fs::read_to_string(&path).ok()?;
    VerbData::from_toml(&content)
}

fn edit_verb_simple_gui(gui_tool: &std::path::Path, data: &mut VerbData) -> bool {
    // Simple mode: just ask for title, publisher, and installer path
    // Name is derived from title, year is current year, category defaults to app
    
    let output = std::process::Command::new(gui_tool)
        .args([
            "--forms",
            "--title", "Create Custom Verb (Simple)",
            "--text", "Enter verb details:\n(Name will be derived from title)",
            "--add-entry", "Title",
            "--add-entry", "Publisher",
            "--add-entry", "Installer Arguments",
            "--width", "500",
        ])
        .output();
    
    if let Ok(out) = output {
        if !out.status.success() {
            return false;
        }
        
        let output_str = String::from_utf8_lossy(&out.stdout).to_string();
        let values: Vec<String> = output_str.trim().split('|').map(|s| s.to_string()).collect();
        
        if values.len() >= 3 {
            data.title = values[0].clone();
            data.publisher = values[1].clone();
            data.installer_args = values[2].clone();
            data.derive_name_from_title();
        }
    } else {
        return false;
    }
    
    // Select installer file
    let output = std::process::Command::new(gui_tool)
        .args([
            "--file-selection",
            "--title", "Select installer executable",
            "--file-filter", "Executables | *.exe *.msi",
        ])
        .output();
    
    if let Ok(out) = output {
        if out.status.success() {
            data.installer_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        } else {
            return false;
        }
    } else {
        return false;
    }
    
    !data.title.is_empty() && !data.installer_path.is_empty()
}

fn edit_verb_advanced_gui(gui_tool: &std::path::Path, data: &mut VerbData) -> bool {
    // Advanced mode: full control over all fields
    
    // First, select category
    let output = std::process::Command::new(gui_tool)
        .args([
            "--list",
            "--title", "Select Category",
            "--column", "Category",
            "--column", "Description",
            "--print-column", "1",
            "--width", "400",
            "--height", "300",
            "app", "Application",
            "dll", "DLL/Runtime",
            "font", "Font",
            "setting", "Setting/Configuration",
            "custom", "Custom/Other",
        ])
        .output();
    
    if let Ok(out) = output {
        if out.status.success() {
            data.category = String::from_utf8_lossy(&out.stdout).trim().to_string();
        } else {
            return false;
        }
    } else {
        return false;
    }
    
    // Select action type
    let output = std::process::Command::new(gui_tool)
        .args([
            "--list",
            "--title", "Select Action Type",
            "--column", "Type",
            "--column", "Description",
            "--print-column", "1",
            "--width", "500",
            "--height", "300",
            "local_installer", "Run a local installer file",
            "script", "Run a shell script",
            "override", "Set DLL override",
            "registry", "Import registry settings",
        ])
        .output();
    
    if let Ok(out) = output {
        if out.status.success() {
            data.action_type = String::from_utf8_lossy(&out.stdout).trim().to_string();
        } else {
            return false;
        }
    } else {
        return false;
    }
    
    // Form for all text fields
    let output = std::process::Command::new(gui_tool)
        .args([
            "--forms",
            "--title", "Create Custom Verb (Advanced)",
            "--text", "Enter verb details:",
            "--add-entry", &format!("Name [{}]", data.name),
            "--add-entry", &format!("Title [{}]", data.title),
            "--add-entry", &format!("Publisher [{}]", data.publisher),
            "--add-entry", &format!("Year [{}]", data.year),
            "--add-entry", &format!("Arguments [{}]", data.installer_args),
            "--width", "500",
        ])
        .output();
    
    if let Ok(out) = output {
        if !out.status.success() {
            return false;
        }
        
        let output_str = String::from_utf8_lossy(&out.stdout).to_string();
        let values: Vec<String> = output_str.trim().split('|').map(|s| s.to_string()).collect();
        
        if values.len() >= 5 {
            if !values[0].is_empty() { data.name = values[0].clone(); }
            if !values[1].is_empty() { data.title = values[1].clone(); }
            if !values[2].is_empty() { data.publisher = values[2].clone(); }
            if !values[3].is_empty() { data.year = values[3].clone(); }
            if !values[4].is_empty() { data.installer_args = values[4].clone(); }
        }
    } else {
        return false;
    }
    
    // Select file based on action type
    let file_title = match data.action_type.as_str() {
        "local_installer" => "Select installer executable",
        "script" => "Select shell script",
        _ => "Select file",
    };
    
    let file_filter = match data.action_type.as_str() {
        "local_installer" => "Executables | *.exe *.msi",
        "script" => "Shell scripts | *.sh",
        _ => "All files | *",
    };
    
    if data.action_type == "local_installer" || data.action_type == "script" {
        let output = std::process::Command::new(gui_tool)
            .args([
                "--file-selection",
                "--title", file_title,
                "--file-filter", file_filter,
            ])
            .output();
        
        if let Ok(out) = output {
            if out.status.success() {
                data.installer_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    
    !data.name.is_empty() && !data.title.is_empty()
}

fn save_verb_gui(gui_tool: &std::path::Path, data: &VerbData) {
    let toml_content = data.to_toml();
    let default_dir = crate::wine::custom::get_custom_verbs_dir();
    
    // Ensure the directory exists
    std::fs::create_dir_all(&default_dir).ok();
    
    // Ask Save or Save As
    let output = std::process::Command::new(gui_tool)
        .args([
            "--list",
            "--title", "Save Verb",
            "--column", "Option",
            "--column", "Description",
            "--print-column", "1",
            "--width", "500",
            "--height", "200",
            "save", &format!("Save to default location (~/.config/protontool/verbs/{}.toml)", data.name),
            "saveas", "Save As... (choose location)",
        ])
        .output();
    
    let save_path = if let Ok(out) = output {
        if !out.status.success() {
            return;
        }
        
        let choice = String::from_utf8_lossy(&out.stdout).trim().to_string();
        
        if choice == "saveas" {
            // Let user choose location
            let output = std::process::Command::new(gui_tool)
                .args([
                    "--file-selection",
                    "--save",
                    "--title", "Save verb as...",
                    "--filename", &format!("{}.toml", data.name),
                    "--file-filter", "TOML files | *.toml",
                ])
                .output();
            
            if let Ok(out) = output {
                if out.status.success() {
                    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !path.is_empty() {
                        PathBuf::from(path)
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            } else {
                return;
            }
        } else {
            // Save to default location
            default_dir.join(format!("{}.toml", data.name))
        }
    } else {
        return;
    };
    
    // Write the file
    match std::fs::write(&save_path, &toml_content) {
        Ok(_) => {
            println!("Verb saved to: {}", save_path.display());
            let _ = std::process::Command::new(gui_tool)
                .args([
                    "--info",
                    "--title", "Verb Saved",
                    "--text", &format!("Custom verb '{}' saved successfully!\n\nLocation: {}\n\nRestart protontool to use the new verb.", data.name, save_path.display()),
                    "--width", "500",
                ])
                .status();
        }
        Err(e) => {
            eprintln!("Failed to save verb: {}", e);
            let _ = std::process::Command::new(gui_tool)
                .args([
                    "--error",
                    "--title", "Save Failed",
                    "--text", &format!("Failed to save verb: {}", e),
                    "--width", "400",
                ])
                .status();
        }
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
    let verb_runner = Wine::new(&proton_app, prefix_path);

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
    let wine_ctx = crate::wine::WineContext::from_proton(&proton_app, prefix_path);

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

fn run_prefix_command_mode(prefix_path: &str, command: &str, parsed: &util::ParsedArgs, no_term: bool) {
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

    // Try to read saved Proton and arch info from prefix metadata
    let metadata_path = prefix_path.join(".protontool");
    let metadata_content = std::fs::read_to_string(&metadata_path).ok();
    
    let proton_app = if let Some(ref metadata) = metadata_content {
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
    
    // Read saved architecture (default to win64)
    let saved_arch = metadata_content.as_ref()
        .and_then(|m| m.lines().find(|l| l.starts_with("arch=")))
        .and_then(|l| l.strip_prefix("arch="))
        .and_then(crate::wine::WineArch::from_str)
        .unwrap_or(crate::wine::WineArch::Win64);

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
        match select_proton_with_gui(&get_proton_apps(&steam_apps)) {
            Some(app) => app,
            None => {
                exit_with_error("No Proton version selected.", no_term);
            }
        }
    };

    if !proton_app.is_proton_ready {
        exit_with_error("Proton installation is not ready.", no_term);
    }

    let wine_ctx = crate::wine::WineContext::from_proton_with_arch(&proton_app, &prefix_path, saved_arch);

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
    
    // Parse architecture option (default to win64)
    let arch = parsed.get_option("arch")
        .and_then(|s| crate::wine::WineArch::from_str(s))
        .unwrap_or(crate::wine::WineArch::Win64);
    
    // Create the prefix directory structure
    println!("Creating Wine prefix at: {}", prefix_path.display());
    println!("Using Proton: {}", proton_app.name);
    println!("Architecture: {}", arch.as_str());

    if let Err(e) = std::fs::create_dir_all(&prefix_path) {
        exit_with_error(&format!("Failed to create prefix directory: {}", e), no_term);
    }

    // Initialize the prefix with Proton's wine
    let wine_ctx = crate::wine::WineContext::from_proton_with_arch(&proton_app, &prefix_path, arch);
    
    // Run wineboot to initialize the prefix
    println!("Initializing prefix with wineboot...");
    match wine_ctx.run_wine_no_cwd(&["wineboot", "--init"]) {
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
        "proton_name={}\nproton_path={}\narch={}\ncreated={}\n",
        proton_app.name,
        proton_app.install_path.display(),
        arch.as_str(),
        chrono_lite_now()
    );
    std::fs::write(&metadata_path, metadata).ok();

    println!("\nPrefix created successfully!");
    println!("\nTo use this prefix:");
    println!("  protontool --prefix '{}' <verbs>", prefix_path.display());
    println!("  protontool --prefix '{}' -c <command>", prefix_path.display());
}

fn run_delete_prefix_mode(prefix_path: &str, no_term: bool) {
    let prefix_path = PathBuf::from(prefix_path);
    
    if !prefix_path.exists() {
        exit_with_error(&format!("Prefix path does not exist: {}", prefix_path.display()), no_term);
    }
    
    let prefix_name = prefix_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown");
    
    // Confirm deletion
    println!("Are you sure you want to delete the prefix '{}'?", prefix_name);
    println!("Path: {}", prefix_path.display());
    println!();
    print!("Type 'yes' to confirm: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        exit_with_error("Failed to read input.", no_term);
    }
    
    if input.trim().to_lowercase() != "yes" {
        println!("Deletion cancelled.");
        return;
    }
    
    // Delete the prefix directory
    match std::fs::remove_dir_all(&prefix_path) {
        Ok(()) => {
            println!("Prefix '{}' deleted successfully.", prefix_name);
        }
        Err(e) => {
            exit_with_error(&format!("Failed to delete prefix: {}", e), no_term);
        }
    }
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

    // Try to read saved Proton and arch info from prefix metadata
    let metadata_path = prefix_path.join(".protontool");
    let metadata_content = std::fs::read_to_string(&metadata_path).ok();
    
    let proton_app = if let Some(ref metadata) = metadata_content {
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
    
    // Read saved architecture (default to win64)
    let saved_arch = metadata_content.as_ref()
        .and_then(|m| m.lines().find(|l| l.starts_with("arch=")))
        .and_then(|l| l.strip_prefix("arch="))
        .and_then(crate::wine::WineArch::from_str)
        .unwrap_or(crate::wine::WineArch::Win64);

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

    let verb_runner = Wine::new_with_arch(&proton_app, &prefix_path, saved_arch);

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
