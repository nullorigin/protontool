use std::fs;
use std::path::{Path, PathBuf};

use crate::vdf::{parse_vdf, VDFDict};

/// Detect if running on a Steam Deck by checking the board name.
pub fn is_steam_deck() -> bool {
    if let Ok(board) = fs::read_to_string("/sys/class/dmi/id/board_name") {
        return board.trim() == "Jupiter";
    }
    false
}

/// Detect if running on SteamOS by checking /etc/os-release.
pub fn is_steamos() -> bool {
    let os_release = Path::new("/etc/os-release");
    if let Ok(content) = fs::read_to_string(os_release) {
        for line in content.lines() {
            if line.starts_with("ID=") {
                let id = line.trim_start_matches("ID=").trim_matches('"');
                return id == "steamos";
            }
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct SteamInstallation {
    pub steam_path: PathBuf,
    pub steam_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SteamApp {
    pub name: String,
    pub appid: u32,
    pub prefix_path: Option<PathBuf>,
    pub install_path: PathBuf,
    pub is_proton: bool,
}

impl SteamApp {
    pub fn is_windows_app(&self) -> bool {
        self.prefix_path.is_some() && !self.is_proton
    }

    pub fn prefix_path_exists(&self) -> bool {
        self.prefix_path.as_ref().map_or(false, |p| p.exists())
    }

    pub fn name_contains(&self, query: &str) -> bool {
        self.name.to_lowercase().contains(&query.to_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct ProtonApp {
    pub name: String,
    pub appid: u32,
    pub install_path: PathBuf,
    pub is_proton_ready: bool,
}

/// Find all Steam installations on the system.
/// Checks common paths and STEAM_DIR environment variable.
pub fn find_steam_installations() -> Vec<SteamInstallation> {
    let mut installations = Vec::new();

    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return installations,
    };

    let candidates = [
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.steam/steam"),
    ];

    for candidate in &candidates {
        if candidate.join("steamapps").exists() {
            installations.push(SteamInstallation {
                steam_path: candidate.clone(),
                steam_root: candidate.clone(),
            });
        }
    }

    if let Ok(steam_dir) = std::env::var("STEAM_DIR") {
        let path = PathBuf::from(&steam_dir);
        if path.join("steamapps").exists() {
            installations.insert(
                0,
                SteamInstallation {
                    steam_path: path.clone(),
                    steam_root: path,
                },
            );
        }
    }

    installations
}

/// Get all Steam library paths from libraryfolders.vdf and extra sources.
/// Includes paths from STEAM_EXTRA_COMPAT_TOOLS_PATHS environment variable.
pub fn get_steam_lib_paths(steam_path: &Path, extra_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut lib_paths = Vec::new();

    let libraryfolders_path = steam_path.join("steamapps/libraryfolders.vdf");

    if let Ok(vdf) = parse_vdf(&libraryfolders_path) {
        if let Some(libraryfolders) = vdf.get_dict("libraryfolders") {
            for (key, value) in libraryfolders.iter() {
                if key.parse::<u32>().is_ok() {
                    if let crate::vdf::VDFValue::Dict(folder_dict) = value {
                        if let Some(path) = folder_dict.get("path") {
                            let lib_path = PathBuf::from(path);
                            if lib_path.exists() && !lib_paths.contains(&lib_path) {
                                lib_paths.push(lib_path);
                            }
                        }
                    }
                }
            }
        }
    }

    let steamapps = steam_path.join("steamapps");
    if steamapps.exists() && !lib_paths.iter().any(|p| p == steam_path) {
        lib_paths.insert(0, steam_path.to_path_buf());
    }

    // Add extra library paths from CLI or environment
    for extra in extra_paths {
        if extra.join("steamapps").exists() && !lib_paths.contains(extra) {
            lib_paths.push(extra.clone());
        }
    }

    // Also check STEAM_EXTRA_COMPAT_TOOLS_PATHS environment variable
    if let Ok(extra_env) = std::env::var("STEAM_EXTRA_COMPAT_TOOLS_PATHS") {
        for path_str in extra_env.split(':') {
            let path = PathBuf::from(path_str);
            if path.join("steamapps").exists() && !lib_paths.contains(&path) {
                lib_paths.push(path);
            }
        }
    }

    lib_paths
}

/// Scan all Steam library paths and parse app manifests.
/// Returns a list of all installed Steam apps with their metadata.
pub fn get_steam_apps(
    steam_root: &Path,
    _steam_path: &Path,
    steam_lib_paths: &[PathBuf],
) -> Vec<SteamApp> {
    let mut apps = Vec::new();

    for lib_path in steam_lib_paths {
        let steamapps = lib_path.join("steamapps");
        let common = steamapps.join("common");

        if let Ok(entries) = fs::read_dir(&steamapps) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("appmanifest_") && name.ends_with(".acf") {
                        // Pass both the library path and steam_root for compatdata lookup
                        if let Some(app) = parse_app_manifest(&path, &common, lib_path, steam_root)
                        {
                            apps.push(app);
                        }
                    }
                }
            }
        }
    }

    apps
}

/// Parse a single appmanifest_*.acf file into a SteamApp.
/// Resolves the prefix path by checking both library and root compatdata.
fn parse_app_manifest(
    manifest_path: &Path,
    common_path: &Path,
    lib_path: &Path,
    steam_root: &Path,
) -> Option<SteamApp> {
    let vdf = parse_vdf(manifest_path).ok()?;
    let app_state = vdf.get_dict("AppState")?;

    let appid: u32 = app_state.get("appid")?.parse().ok()?;
    let name = app_state.get("name")?.to_string();
    let installdir = app_state.get("installdir")?;

    let install_path = common_path.join(installdir);

    let is_proton = name.starts_with("Proton");

    let prefix_path = if !is_proton {
        // First check in the same library where the game is installed
        let lib_compat_path = lib_path
            .join("steamapps/compatdata")
            .join(appid.to_string())
            .join("pfx");
        if lib_compat_path.exists() {
            Some(lib_compat_path)
        } else {
            // Fall back to steam_root compatdata
            let root_compat_path = steam_root
                .join("steamapps/compatdata")
                .join(appid.to_string())
                .join("pfx");
            if root_compat_path.exists() {
                Some(root_compat_path)
            } else {
                None
            }
        }
    } else {
        None
    };

    Some(SteamApp {
        name,
        appid,
        prefix_path,
        install_path,
        is_proton,
    })
}

/// Find the Proton version configured for a specific app.
/// Checks PROTON_VERSION env var, then Steam config, then falls back to newest.
pub fn find_proton_app(
    steam_path: &Path,
    steam_apps: &[SteamApp],
    appid: u32,
) -> Option<ProtonApp> {
    let proton_version = std::env::var("PROTON_VERSION").ok();

    let _target_app = steam_apps.iter().find(|app| app.appid == appid)?;

    let config_path = steam_path.join("config/config.vdf");
    let mut selected_proton: Option<&SteamApp> = None;

    if let Some(ref version_name) = proton_version {
        selected_proton = steam_apps
            .iter()
            .find(|app| app.is_proton && app.name == *version_name);
    }

    if selected_proton.is_none() {
        if let Ok(config_vdf) = parse_vdf(&config_path) {
            if let Some(compat_tool) = find_compat_tool_for_app(&config_vdf, appid) {
                selected_proton = steam_apps
                    .iter()
                    .find(|app| app.is_proton && app.name.contains(&compat_tool));
            }
        }
    }

    if selected_proton.is_none() {
        selected_proton = steam_apps
            .iter()
            .filter(|app| app.is_proton)
            .max_by(|a, b| a.name.cmp(&b.name));
    }

    let proton = selected_proton?;

    let proton_dist = proton.install_path.join("dist");
    let proton_files = proton.install_path.join("files");
    let is_ready = proton_dist.exists() || proton_files.exists();

    Some(ProtonApp {
        name: proton.name.clone(),
        appid: proton.appid,
        install_path: proton.install_path.clone(),
        is_proton_ready: is_ready,
    })
}

/// Look up the compatibility tool name configured for an app in config.vdf.
fn find_compat_tool_for_app(config_vdf: &VDFDict, appid: u32) -> Option<String> {
    let software = config_vdf
        .get_dict("InstallConfigStore")?
        .get_dict("Software")?;

    let valve = software
        .get_dict("Valve")
        .or_else(|| software.get_dict("valve"))?;

    let steam = valve
        .get_dict("Steam")
        .or_else(|| valve.get_dict("steam"))?;

    let compat_mapping = steam.get_dict("CompatToolMapping")?;
    let app_config = compat_mapping.get_dict(&appid.to_string())?;

    app_config.get("name").map(|s| s.to_string())
}

/// Find the legacy Steam Runtime (ubuntu12_32) path if it exists.
pub fn find_legacy_steam_runtime_path(steam_root: &Path) -> Option<PathBuf> {
    let runtime_path = steam_root.join("ubuntu12_32/steam-runtime");
    if runtime_path.exists() {
        Some(runtime_path)
    } else {
        None
    }
}

/// Get all available Proton installations (deduplicated by appid)
pub fn get_proton_apps(steam_apps: &[SteamApp]) -> Vec<ProtonApp> {
    let mut seen_appids = std::collections::HashSet::new();
    steam_apps
        .iter()
        .filter(|app| app.is_proton)
        .filter(|app| seen_appids.insert(app.appid)) // Only keep first occurrence
        .map(|app| {
            let proton_dist = app.install_path.join("dist");
            let proton_files = app.install_path.join("files");
            let is_ready = proton_dist.exists() || proton_files.exists();

            ProtonApp {
                name: app.name.clone(),
                appid: app.appid,
                install_path: app.install_path.clone(),
                is_proton_ready: is_ready,
            }
        })
        .collect()
}

/// Find a specific Proton app by name
pub fn find_proton_by_name(steam_apps: &[SteamApp], name: &str) -> Option<ProtonApp> {
    let app = steam_apps
        .iter()
        .find(|app| app.is_proton && app.name.to_lowercase().contains(&name.to_lowercase()))?;

    let proton_dist = app.install_path.join("dist");
    let proton_files = app.install_path.join("files");
    let is_ready = proton_dist.exists() || proton_files.exists();

    Some(ProtonApp {
        name: app.name.clone(),
        appid: app.appid,
        install_path: app.install_path.clone(),
        is_proton_ready: is_ready,
    })
}
