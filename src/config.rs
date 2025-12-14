use std::env;
use std::path::PathBuf;

#[cfg(not(feature = "custom_steam_dir"))]
pub const DEFAULT_STEAM_DIR: Option<&str> = None;

#[cfg(feature = "custom_steam_dir")]
pub const DEFAULT_STEAM_DIR: Option<&str> = option_env!("protontool_DEFAULT_STEAM_DIR");

#[cfg(not(feature = "custom_gui_provider"))]
pub const DEFAULT_GUI_PROVIDER: Option<&str> = None;

#[cfg(feature = "custom_gui_provider")]
pub const DEFAULT_GUI_PROVIDER: Option<&str> = option_env!("protontool_DEFAULT_GUI_PROVIDER");

#[cfg(not(feature = "custom_steam_runtime"))]
pub const DEFAULT_STEAM_RUNTIME_PATH: Option<&str> = None;

#[cfg(feature = "custom_steam_runtime")]
pub const DEFAULT_STEAM_RUNTIME_PATH: Option<&str> = option_env!("protontool_STEAM_RUNTIME_PATH");

pub mod defaults {
    pub const STEAM_CANDIDATES: &[&str] = &[
        ".steam/root",
        ".steam/steam", 
        ".local/share/Steam",
    ];

    pub const PROTON_PREFIXES: &[&str] = &[
        "Proton",
        "Proton - Experimental",
        "Proton Experimental", 
        "Proton Hotfix",
    ];

    pub const GUI_PROVIDERS: &[&str] = &["yad", "zenity"];
}

pub fn get_config_dir() -> PathBuf {
    if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg_config).join("protontool")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".config/protontool")
    } else {
        PathBuf::from("/tmp/protontool")
    }
}

pub fn get_cache_dir() -> PathBuf {
    if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg_cache).join("protontool")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".cache/protontool")
    } else {
        PathBuf::from("/tmp/protontool-cache")
    }
}

pub fn get_steam_dir() -> Option<PathBuf> {
    if let Ok(steam_dir) = env::var("STEAM_DIR") {
        return Some(PathBuf::from(steam_dir));
    }
    
    DEFAULT_STEAM_DIR.map(PathBuf::from)
}

pub fn get_gui_provider() -> Option<String> {
    if let Ok(provider) = env::var("protontool_GUI") {
        return Some(provider);
    }
    
    DEFAULT_GUI_PROVIDER.map(String::from)
}

pub fn get_steam_runtime_override() -> Option<PathBuf> {
    if let Ok(runtime) = env::var("STEAM_RUNTIME") {
        if runtime != "0" && runtime != "1" && !runtime.is_empty() {
            return Some(PathBuf::from(runtime));
        }
    }
    
    DEFAULT_STEAM_RUNTIME_PATH.map(PathBuf::from)
}

pub fn is_steam_runtime_disabled() -> bool {
    env::var("STEAM_RUNTIME").map(|v| v == "0").unwrap_or(false)
}
