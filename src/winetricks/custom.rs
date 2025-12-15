use std::path::{Path, PathBuf};
use std::fs;

use super::verbs::{Verb, VerbCategory, VerbAction, LocalFile};

/// Loads custom verbs from the user's config directory.
/// 
/// Custom verbs can be defined in two ways:
/// 
/// 1. **Shell scripts**: Place a `.sh` file in `~/.config/protontool/verbs/`
///    The script will be executed with environment variables set:
///    - WINEPREFIX, WINE, WINESERVER, PROTON_PATH
///    - W_TMP, W_CACHE, W_SYSTEM32_DLLS, W_SYSTEM64_DLLS
/// 
/// 2. **TOML definitions**: Place a `.toml` file in `~/.config/protontool/verbs/`
///    for declarative verb definitions supporting local installers.
/// 
/// Example TOML (sketchup.toml):
/// ```toml
/// [verb]
/// name = "sketchup2024"
/// category = "app"
/// title = "SketchUp 2024"
/// publisher = "Trimble"
/// year = "2024"
/// 
/// [[actions]]
/// type = "local_installer"
/// path = "~/Downloads/SketchUpPro-2024.exe"
/// args = ["/S"]
/// ```

pub fn get_custom_verbs_dir() -> PathBuf {
    crate::config::get_verbs_dir()
}

pub fn load_custom_verbs() -> Vec<Verb> {
    let verbs_dir = get_custom_verbs_dir();
    if !verbs_dir.exists() {
        return Vec::new();
    }

    let mut verbs = Vec::new();

    if let Ok(entries) = fs::read_dir(&verbs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "sh" {
                    if let Some(verb) = load_script_verb(&path) {
                        verbs.push(verb);
                    }
                } else if ext == "toml" {
                    if let Some(verb) = load_toml_verb(&path) {
                        verbs.push(verb);
                    }
                }
            }
        }
    }

    verbs
}

/// Load a verb from a shell script file.
/// The verb name is derived from the filename (without extension).
fn load_script_verb(script_path: &Path) -> Option<Verb> {
    let name = script_path.file_stem()?.to_string_lossy().to_string();
    
    // Try to extract metadata from script comments
    let content = fs::read_to_string(script_path).ok()?;
    let (title, publisher, year) = parse_script_metadata(&content, &name);
    
    Some(Verb::new(&name, VerbCategory::Custom, &title, &publisher, &year)
        .with_actions(vec![VerbAction::RunScript { script_path: script_path.to_path_buf() }]))
}

/// Parse metadata from script header comments.
/// Looks for lines like:
/// # Title: My Application
/// # Publisher: Some Company
/// # Year: 2024
fn parse_script_metadata(content: &str, default_name: &str) -> (String, String, String) {
    let mut title = default_name.to_string();
    let mut publisher = String::new();
    let mut year = String::new();

    for line in content.lines().take(20) {
        let line = line.trim();
        if !line.starts_with('#') {
            continue;
        }
        let line = line.trim_start_matches('#').trim();
        
        if let Some(val) = line.strip_prefix("Title:") {
            title = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Publisher:") {
            publisher = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Year:") {
            year = val.trim().to_string();
        }
    }

    (title, publisher, year)
}

/// Load a verb from a TOML definition file.
fn load_toml_verb(toml_path: &Path) -> Option<Verb> {
    let content = fs::read_to_string(toml_path).ok()?;
    parse_toml_verb(&content)
}

/// Parse a TOML verb definition.
/// 
/// Simple parser that doesn't require external dependencies.
fn parse_toml_verb(content: &str) -> Option<Verb> {
    let mut name = String::new();
    let mut category = VerbCategory::App;
    let mut title = String::new();
    let mut publisher = String::new();
    let mut year = String::new();
    let mut actions: Vec<VerbAction> = Vec::new();

    let mut in_verb_section = false;
    let mut in_action_section = false;
    let mut current_action_type = String::new();
    let mut current_action_path = String::new();
    let mut current_action_args: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line == "[verb]" {
            in_verb_section = true;
            in_action_section = false;
            continue;
        }

        if line == "[[actions]]" {
            // Save previous action if any
            if !current_action_type.is_empty() {
                if let Some(action) = create_action(&current_action_type, &current_action_path, &current_action_args) {
                    actions.push(action);
                }
            }
            in_verb_section = false;
            in_action_section = true;
            current_action_type.clear();
            current_action_path.clear();
            current_action_args.clear();
            continue;
        }

        if let Some((key, value)) = parse_toml_line(line) {
            if in_verb_section {
                match key.as_str() {
                    "name" => name = value,
                    "category" => category = parse_category(&value),
                    "title" => title = value,
                    "publisher" => publisher = value,
                    "year" => year = value,
                    _ => {}
                }
            } else if in_action_section {
                match key.as_str() {
                    "type" => current_action_type = value,
                    "path" => current_action_path = expand_path(&value),
                    "args" => current_action_args = parse_string_array(&value),
                    "dll" => current_action_path = value, // reuse path for dll name
                    "mode" => current_action_args = vec![value], // reuse args for mode
                    "content" => current_action_path = value, // reuse for registry content
                    _ => {}
                }
            }
        }
    }

    // Save last action
    if !current_action_type.is_empty() {
        if let Some(action) = create_action(&current_action_type, &current_action_path, &current_action_args) {
            actions.push(action);
        }
    }

    if name.is_empty() {
        return None;
    }

    if title.is_empty() {
        title = name.clone();
    }

    Some(Verb::new(&name, category, &title, &publisher, &year).with_actions(actions))
}

fn parse_toml_line(line: &str) -> Option<(String, String)> {
    let mut parts = line.splitn(2, '=');
    let key = parts.next()?.trim().to_string();
    let value = parts.next()?.trim();
    
    // Remove quotes from string values
    let value = value.trim_matches('"').to_string();
    
    Some((key, value))
}

fn parse_category(s: &str) -> VerbCategory {
    match s.to_lowercase().as_str() {
        "app" | "apps" => VerbCategory::App,
        "dll" | "dlls" => VerbCategory::Dll,
        "font" | "fonts" => VerbCategory::Font,
        "setting" | "settings" => VerbCategory::Setting,
        "custom" => VerbCategory::Custom,
        _ => VerbCategory::Custom, // Default to Custom for user-defined verbs
    }
}

fn expand_path(path: &str) -> String {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen("~", &home, 1);
        }
    }
    path.to_string()
}

fn parse_string_array(s: &str) -> Vec<String> {
    // Simple array parser: ["arg1", "arg2"]
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return vec![s.to_string()];
    }
    
    let inner = &s[1..s.len()-1];
    inner.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn create_action(action_type: &str, path: &str, args: &[String]) -> Option<VerbAction> {
    match action_type {
        "local_installer" => {
            let local_file = LocalFile::new(Path::new(path), path);
            Some(VerbAction::RunLocalInstaller { file: local_file, args: args.to_vec() })
        }
        "script" => {
            Some(VerbAction::RunScript { script_path: PathBuf::from(path) })
        }
        "override" => {
            let mode = args.first().map(|s| s.as_str()).unwrap_or("native");
            let dll_override = match mode {
                "native" => super::verbs::DllOverride::Native,
                "builtin" => super::verbs::DllOverride::Builtin,
                "native,builtin" => super::verbs::DllOverride::NativeBuiltin,
                "builtin,native" => super::verbs::DllOverride::BuiltinNative,
                _ => super::verbs::DllOverride::Native,
            };
            Some(VerbAction::Override { dll: path.to_string(), mode: dll_override })
        }
        "registry" => {
            Some(VerbAction::Registry { content: path.to_string() })
        }
        "winecfg" => {
            Some(VerbAction::Winecfg { args: args.to_vec() })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_toml_verb() {
        let toml = r#"
[verb]
name = "sketchup2024"
category = "app"
title = "SketchUp 2024"
publisher = "Trimble"
year = "2024"

[[actions]]
type = "local_installer"
path = "~/Downloads/SketchUpPro-2024.exe"
args = ["/S"]
"#;
        let verb = parse_toml_verb(toml).unwrap();
        assert_eq!(verb.name, "sketchup2024");
        assert_eq!(verb.title, "SketchUp 2024");
        assert_eq!(verb.publisher, "Trimble");
        assert_eq!(verb.actions.len(), 1);
    }

    #[test]
    fn test_parse_script_metadata() {
        let script = r#"#!/bin/bash
# Title: My Custom App
# Publisher: Some Company
# Year: 2024

$WINE setup.exe /S
"#;
        let (title, publisher, year) = parse_script_metadata(script, "default");
        assert_eq!(title, "My Custom App");
        assert_eq!(publisher, "Some Company");
        assert_eq!(year, "2024");
    }
}
