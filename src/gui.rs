use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config;
use crate::steam::{ProtonApp, SteamApp, SteamInstallation};
use crate::util::which;
use crate::winetricks::{Verb, VerbCategory};

pub fn get_gui_tool() -> Option<std::path::PathBuf> {
    if let Some(provider) = config::get_gui_provider() {
        return which(&provider);
    }
    
    for tool in config::defaults::GUI_PROVIDERS {
        if let Some(path) = which(tool) {
            return Some(path);
        }
    }
    None
}

pub fn show_text_dialog(title: &str, text: &str) {
    if let Some(zenity) = which("zenity") {
        let _ = Command::new(zenity)
            .args(["--text-info", "--title", title, "--width", "800", "--height", "600"])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            });
    } else if let Some(yad) = which("yad") {
        let _ = Command::new(yad)
            .args(["--text-info", "--title", title, "--width", "800", "--height", "600"])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            });
    } else {
        eprintln!("No dialog tool (zenity/yad) available");
        eprintln!("{}", text);
    }
}

pub fn prompt_filesystem_access(_paths: &[&Path], _show_dialog: bool) {
    // On native Linux without Flatpak, no filesystem access prompts are needed
}

/// Prompt user to add additional Steam library paths via GUI.
/// Returns a vector of paths the user selected.
pub fn select_steam_library_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let gui_tool = match get_gui_tool() {
        Some(tool) => tool,
        None => return paths,
    };

    loop {
        // Build list of current paths for display
        let paths_display = if paths.is_empty() {
            "(no additional paths added)".to_string()
        } else {
            paths.iter()
                .map(|p| format!("  • {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Show dialog with Add and Next buttons
        let output = Command::new(&gui_tool)
            .args([
                "--question",
                "--title", "Steam Library Paths",
                "--text", &format!(
                    "Add additional Steam library folders?\n\n\
                     Current paths:\n{}\n",
                    paths_display
                ),
                "--ok-label", "Add Path",
                "--cancel-label", "Next",
                "--width", "500",
            ])
            .status();

        match output {
            Ok(status) if status.success() => {
                // User clicked "Add Path", show directory picker
                let dir_output = Command::new(&gui_tool)
                    .args([
                        "--file-selection",
                        "--directory",
                        "--title", "Select Steam Library Folder (containing 'steamapps')",
                    ])
                    .output();

                if let Ok(out) = dir_output {
                    if out.status.success() {
                        let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !path_str.is_empty() {
                            let path = PathBuf::from(&path_str);
                            
                            // Validate it looks like a Steam library
                            if path.join("steamapps").exists() {
                                if !paths.contains(&path) {
                                    paths.push(path);
                                }
                            } else {
                                // Warn user this doesn't look like a Steam library
                                let _ = Command::new(&gui_tool)
                                    .args([
                                        "--warning",
                                        "--title", "Invalid Path",
                                        "--text", &format!(
                                            "The selected folder doesn't appear to be a Steam library.\n\n\
                                             No 'steamapps' folder found in:\n{}\n\n\
                                             Please select a folder containing a 'steamapps' subdirectory.",
                                            path_str
                                        ),
                                        "--width", "500",
                                    ])
                                    .status();
                            }
                        }
                    }
                }
            }
            _ => {
                // User clicked "Next" or cancelled
                break;
            }
        }
    }

    paths
}

pub fn select_steam_installation(installations: &[SteamInstallation]) -> Option<SteamInstallation> {
    if installations.is_empty() {
        return None;
    }
    
    if installations.len() == 1 {
        return Some(installations[0].clone());
    }
    
    let gui_tool = get_gui_tool()?;
    
    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        "Select Steam installation".to_string(),
        "--column".to_string(),
        "Steam Path".to_string(),
    ];
    
    for inst in installations {
        args.push(inst.steam_path.to_string_lossy().to_string());
    }
    
    let output = Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    installations.iter()
        .find(|inst| inst.steam_path.to_string_lossy() == selected)
        .cloned()
}

pub fn select_steam_app_with_gui(
    steam_apps: &[SteamApp],
    title: Option<&str>,
    _steam_path: &Path,
) -> Option<SteamApp> {
    let gui_tool = get_gui_tool()?;
    
    let title = title.unwrap_or("Select a Steam app");
    
    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        title.to_string(),
        "--column".to_string(),
        "App ID".to_string(),
        "--column".to_string(),
        "Name".to_string(),
        "--print-column".to_string(),
        "1".to_string(),
    ];
    
    let mut windows_apps: Vec<_> = steam_apps.iter()
        .filter(|app| app.is_windows_app())
        .collect();
    
    windows_apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    
    for app in &windows_apps {
        args.push(app.appid.to_string());
        args.push(app.name.clone());
    }
    
    let output = Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected_id: u32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    
    steam_apps.iter()
        .find(|app| app.appid == selected_id)
        .cloned()
}

/// Show a GUI to select verbs to run. Returns list of selected verb names.
pub fn select_verbs_with_gui(verbs: &[&Verb], title: Option<&str>) -> Vec<String> {
    let gui_tool = match get_gui_tool() {
        Some(tool) => tool,
        None => return vec![],
    };
    
    let title = title.unwrap_or("Select components to install");
    
    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        title.to_string(),
        "--checklist".to_string(),
        "--column".to_string(),
        "".to_string(),
        "--column".to_string(),
        "Verb".to_string(),
        "--column".to_string(),
        "Category".to_string(),
        "--column".to_string(),
        "Description".to_string(),
        "--separator".to_string(),
        " ".to_string(),
        "--print-column".to_string(),
        "2".to_string(),
        "--width".to_string(),
        "800".to_string(),
        "--height".to_string(),
        "600".to_string(),
    ];
    
    for verb in verbs {
        args.push("FALSE".to_string()); // checkbox state
        args.push(verb.name.clone());
        args.push(verb.category.as_str().to_string());
        args.push(verb.title.clone());
    }
    
    let output = match Command::new(&gui_tool).args(&args).output() {
        Ok(out) => out,
        Err(_) => return vec![],
    };
    
    if !output.status.success() {
        return vec![];
    }
    
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// Show a category selection menu first, then verbs in that category
pub fn select_verb_category_gui() -> Option<VerbCategory> {
    let gui_tool = get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "Select category",
        "--column", "Category",
        "--column", "Description",
        "--print-column", "1",
        "dlls", "Install Windows DLLs and components",
        "fonts", "Install fonts",
        "settings", "Change Wine settings",
        "apps", "Install applications",
    ];
    
    let output = Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    match selected.as_str() {
        "dlls" => Some(VerbCategory::Dll),
        "fonts" => Some(VerbCategory::Font),
        "settings" => Some(VerbCategory::Setting),
        "apps" => Some(VerbCategory::App),
        _ => None,
    }
}

/// Show a GUI to select a Proton version from available installations
pub fn select_proton_with_gui(proton_apps: &[ProtonApp]) -> Option<ProtonApp> {
    let gui_tool = get_gui_tool()?;
    
    if proton_apps.is_empty() {
        let _ = Command::new(&gui_tool)
            .args([
                "--error",
                "--title", "No Proton Found",
                "--text", "No Proton installations were found.\n\nPlease install Proton through Steam first.",
                "--width", "400",
            ])
            .status();
        return None;
    }
    
    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        "Select Proton version".to_string(),
        "--column".to_string(),
        "Name".to_string(),
        "--column".to_string(),
        "Status".to_string(),
        "--print-column".to_string(),
        "1".to_string(),
        "--width".to_string(),
        "500".to_string(),
        "--height".to_string(),
        "400".to_string(),
    ];
    
    let mut sorted_apps: Vec<_> = proton_apps.iter().collect();
    sorted_apps.sort_by(|a, b| b.name.cmp(&a.name)); // Newest first
    
    for app in &sorted_apps {
        args.push(app.name.clone());
        args.push(if app.is_proton_ready { "Ready".to_string() } else { "Not initialized".to_string() });
    }
    
    let output = Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    proton_apps.iter()
        .find(|app| app.name == selected_name)
        .cloned()
}

/// Show a GUI to get a name for a new prefix
pub fn get_prefix_name_gui() -> Option<String> {
    let gui_tool = get_gui_tool()?;
    
    let output = Command::new(&gui_tool)
        .args([
            "--entry",
            "--title", "Create New Prefix",
            "--text", "Enter a name for the new Wine prefix:",
            "--entry-text", "MyPrefix",
            "--width", "400",
        ])
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Show a GUI to select a directory for the new prefix
pub fn select_prefix_location_gui(default_name: &str) -> Option<PathBuf> {
    let gui_tool = get_gui_tool()?;
    
    // First ask if they want the default location or custom
    let prefixes_dir = crate::config::get_prefixes_dir();
    let default_path = prefixes_dir.join(default_name).to_string_lossy().to_string();
    
    let question = Command::new(&gui_tool)
        .args([
            "--question",
            "--title", "Prefix Location",
            "--text", &format!(
                "Use default location for prefix?\n\n{}\n\nClick Yes for default, No to choose a custom location.",
                default_path
            ),
            "--width", "500",
        ])
        .status();
    
    match question {
        Ok(status) if status.success() => {
            // User wants default location
            Some(PathBuf::from(&default_path))
        }
        _ => {
            // User wants to pick custom location
            let output = Command::new(&gui_tool)
                .args([
                    "--file-selection",
                    "--directory",
                    "--save",
                    "--title", "Select location for new prefix",
                    "--filename", &format!("{}/", prefixes_dir.display()),
                ])
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
    }
}

/// Show main menu for GUI mode - returns the selected action
pub enum GuiAction {
    ManageGame,
    CreatePrefix,
    ManagePrefix,
}

pub fn show_main_menu_gui() -> Option<GuiAction> {
    let gui_tool = get_gui_tool()?;
    
    let args = vec![
        "--list",
        "--title", "protontool",
        "--text", "What would you like to do?",
        "--column", "Action",
        "--column", "Description",
        "--print-column", "1",
        "--width", "500",
        "--height", "300",
        "game", "Manage a Steam game prefix",
        "create", "Create a new custom prefix",
        "prefix", "Manage an existing custom prefix",
    ];
    
    let output = Command::new(&gui_tool)
        .args(&args)
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    match selected.as_str() {
        "game" => Some(GuiAction::ManageGame),
        "create" => Some(GuiAction::CreatePrefix),
        "prefix" => Some(GuiAction::ManagePrefix),
        _ => None,
    }
}

/// Show a GUI to select from existing custom prefixes
pub fn select_custom_prefix_gui(prefixes_dir: &Path) -> Option<PathBuf> {
    let gui_tool = get_gui_tool()?;
    
    // List subdirectories in the prefixes directory
    let entries: Vec<_> = std::fs::read_dir(prefixes_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    
    if entries.is_empty() {
        let _ = Command::new(&gui_tool)
            .args([
                "--info",
                "--title", "No Prefixes Found",
                "--text", "No custom prefixes found.\n\nUse 'Create a new custom prefix' to create one.",
                "--width", "400",
            ])
            .status();
        return None;
    }
    
    let mut args = vec![
        "--list".to_string(),
        "--title".to_string(),
        "Select a custom prefix".to_string(),
        "--column".to_string(),
        "Name".to_string(),
        "--column".to_string(),
        "Path".to_string(),
        "--print-column".to_string(),
        "2".to_string(),
        "--width".to_string(),
        "600".to_string(),
        "--height".to_string(),
        "400".to_string(),
    ];
    
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().to_string_lossy().to_string();
        args.push(name);
        args.push(path);
    }
    
    let output = Command::new(&gui_tool)
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
        Some(PathBuf::from(selected))
    }
}
