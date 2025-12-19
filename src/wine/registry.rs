use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use super::WineContext;

/// Registry keys that should have paths filtered out during prefix init
pub const FILTER_REGISTRY_KEYS: &[&str] = &[
    r"Software\Microsoft\Windows\CurrentVersion\Fonts",
    r"Software\Microsoft\Windows NT\CurrentVersion\Fonts",
    r"Software\Wine\Fonts\External Fonts",
];

/// Parse a registry key line like "[Software\\Key] 12345"
/// Returns the key path if this is a valid key line.
///
/// ```
/// use protontool::wine::registry::parse_registry_key_line;
/// assert_eq!(parse_registry_key_line("[Software\\Key] 12345"), Some("Software\\Key"));
/// assert_eq!(parse_registry_key_line("not a key"), None);
/// ```
pub fn parse_registry_key_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return None;
    }

    // Find the closing bracket followed by space and digits
    if let Some(bracket_pos) = trimmed.find("] ") {
        let after_bracket = &trimmed[bracket_pos + 2..];
        // Check if remaining is all digits (timestamp)
        if after_bracket.chars().all(|c| c.is_ascii_digit()) {
            return Some(&trimmed[1..bracket_pos]);
        }
    }
    None
}

/// Parse a registry value line like "\"name\"=\"value\""
/// Returns (name, value) if this is a valid value line.
///
/// ```
/// use protontool::wine::registry::parse_registry_value_line;
/// assert_eq!(parse_registry_value_line(r#""name"="value""#), Some(("name", "value")));
/// assert_eq!(parse_registry_value_line("not a value"), None);
/// ```
pub fn parse_registry_value_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('"') {
        return None;
    }

    // Find the closing quote of the name
    let rest = &trimmed[1..];
    let name_end = rest.find('"')?;
    let name = &rest[..name_end];

    // Skip to the = sign
    let after_name = &rest[name_end + 1..];
    let eq_pos = after_name.find('=')?;
    let after_eq = after_name[eq_pos + 1..].trim();

    // Check if value starts with quote
    if after_eq.starts_with('"') && after_eq.len() > 1 {
        // Find closing quote
        let value_start = &after_eq[1..];
        if let Some(value_end) = value_start.rfind('"') {
            let value = &value_start[..value_end];
            return Some((name, value));
        }
    }

    None
}

/// Filter registry file to remove fully qualified paths from font-related keys
///
/// These paths are specific to the build machine and can cause issues.
pub fn filter_registry_file(filename: &Path, filter_keys: &[&str]) -> std::io::Result<()> {
    let file = File::open(filename)?;
    let reader = BufReader::new(file);

    let tmp_path = filename.with_extension("reg.tmp");
    let mut output = File::create(&tmp_path)?;

    let mut filtering = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Check for registry key header
        if let Some(key) = parse_registry_key_line(trimmed) {
            writeln!(output, "{}", line)?;
            filtering = filter_keys.iter().any(|k| key.contains(k));
            continue;
        }

        // Check for registry value
        if let Some((_, value)) = parse_registry_value_line(trimmed) {
            if filtering {
                // Check if value looks like an absolute path (starts with drive letter)
                if value.len() > 2 && value.chars().nth(1) == Some(':') {
                    // Skip this line - it contains an absolute path
                    continue;
                }
            }
            writeln!(output, "{}", line)?;
            continue;
        }

        // Write all other lines unchanged
        writeln!(output, "{}", line)?;
    }

    // Replace original with filtered version
    fs::rename(&tmp_path, filename)?;

    Ok(())
}

/// Helper for modifying the Windows registry within a Wine prefix.
pub struct RegistryEditor<'a> {
    wine_ctx: &'a WineContext,
}

impl<'a> RegistryEditor<'a> {
    /// Create a new RegistryEditor for the given WineContext.
    pub fn new(wine_ctx: &'a WineContext) -> Self {
        Self { wine_ctx }
    }

    /// Set a registry value with the specified type.
    pub fn set_value(
        &self,
        key: &str,
        name: &str,
        value: &str,
        value_type: RegType,
    ) -> Result<(), String> {
        let reg_content = format!(
            "Windows Registry Editor Version 5.00\n\n[{}]\n\"{}\"={}",
            key,
            name,
            value_type.format_value(value)
        );

        self.apply_reg_content(&reg_content)
    }

    /// Delete a specific registry value.
    pub fn delete_value(&self, key: &str, name: &str) -> Result<(), String> {
        let reg_content = format!(
            "Windows Registry Editor Version 5.00\n\n[{}]\n\"{}\"=-",
            key, name
        );

        self.apply_reg_content(&reg_content)
    }

    /// Delete an entire registry key and all its values.
    pub fn delete_key(&self, key: &str) -> Result<(), String> {
        let reg_content = format!("Windows Registry Editor Version 5.00\n\n[-{}]", key);

        self.apply_reg_content(&reg_content)
    }

    /// Apply a .reg file to the Wine prefix using regedit.
    pub fn apply_reg_file(&self, reg_file: &Path) -> Result<(), String> {
        self.wine_ctx
            .run_regedit(reg_file)
            .map_err(|e| format!("Failed to apply registry file: {}", e))?;
        Ok(())
    }

    /// Write registry content to a temp file and apply it via regedit.
    fn apply_reg_content(&self, content: &str) -> Result<(), String> {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("protontool_reg_patch.reg");

        std::fs::write(&temp_file, content)
            .map_err(|e| format!("Failed to write temp registry file: {}", e))?;

        let result = self
            .wine_ctx
            .run_regedit(&temp_file)
            .map_err(|e| format!("Failed to apply registry patch: {}", e));

        std::fs::remove_file(&temp_file).ok();

        result?;
        Ok(())
    }
}

/// Windows registry value types.
#[derive(Debug, Clone, Copy)]
pub enum RegType {
    String,
    Dword,
    Binary,
    ExpandString,
    MultiString,
}

impl RegType {
    /// Format a value string according to this registry type for .reg file syntax.
    ///
    /// ```
    /// use protontool::wine::registry::RegType;
    /// assert_eq!(RegType::String.format_value("test"), r#""test""#);
    /// assert_eq!(RegType::Dword.format_value("255"), "dword:000000ff");
    /// ```
    pub fn format_value(&self, value: &str) -> String {
        match self {
            RegType::String => format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
            RegType::Dword => {
                if let Ok(num) = value.parse::<u32>() {
                    format!("dword:{:08x}", num)
                } else if value.starts_with("0x") {
                    format!("dword:{}", &value[2..])
                } else {
                    format!("dword:{}", value)
                }
            }
            RegType::Binary => {
                let hex_bytes: Vec<String> = value
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect();
                format!("hex:{}", hex_bytes.join(","))
            }
            RegType::ExpandString => {
                format!("hex(2):{}", Self::string_to_utf16_hex(value))
            }
            RegType::MultiString => {
                format!("hex(7):{}", Self::string_to_utf16_hex(value))
            }
        }
    }

    /// Convert a string to UTF-16LE hex bytes for REG_EXPAND_SZ/REG_MULTI_SZ.
    fn string_to_utf16_hex(s: &str) -> String {
        let utf16: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        utf16
            .iter()
            .flat_map(|&c| vec![(c & 0xFF) as u8, (c >> 8) as u8])
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Set the Windows version reported by Wine to applications.
pub fn set_windows_version(wine_ctx: &WineContext, version: WindowsVersion) -> Result<(), String> {
    let editor = RegistryEditor::new(wine_ctx);

    let (product_name, csd_version, build, build_num, current_version, csd_dword) = match version {
        WindowsVersion::Win11 => ("Microsoft Windows 11", "", "22000", "22000", "6.3", 0u32),
        WindowsVersion::Win10 => ("Microsoft Windows 10", "", "19041", "19041", "6.3", 0),
        WindowsVersion::Win81 => ("Microsoft Windows 8.1", "", "9600", "9600", "6.3", 0),
        WindowsVersion::Win8 => ("Microsoft Windows 8", "", "9200", "9200", "6.2", 0),
        WindowsVersion::Win7 => (
            "Microsoft Windows 7",
            "Service Pack 1",
            "7601",
            "7601",
            "6.1",
            0x100,
        ),
        WindowsVersion::Vista => (
            "Microsoft Windows Vista",
            "Service Pack 2",
            "6002",
            "6002",
            "6.0",
            0x200,
        ),
        WindowsVersion::WinXP => (
            "Microsoft Windows XP",
            "Service Pack 3",
            "2600",
            "2600",
            "5.1",
            0x300,
        ),
    };

    let reg_content = format!(
        r#"Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\Software\Microsoft\Windows NT\CurrentVersion]
"ProductName"="{}"
"CSDVersion"="{}"
"CurrentBuild"="{}"
"CurrentBuildNumber"="{}"
"CurrentVersion"="{}"

[HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Windows]
"CSDVersion"=dword:{:08x}
"#,
        product_name, csd_version, build, build_num, current_version, csd_dword
    );

    editor.apply_reg_content(&reg_content)
}

/// Supported Windows versions for Wine compatibility settings.
#[derive(Debug, Clone, Copy)]
pub enum WindowsVersion {
    Win11,
    Win10,
    Win81,
    Win8,
    Win7,
    Vista,
    WinXP,
}

impl WindowsVersion {
    /// Parse a Windows version from a string (e.g., "win10", "7", "xp").
    ///
    /// ```
    /// use protontool::wine::registry::WindowsVersion;
    /// assert!(WindowsVersion::from_str("win10").is_some());
    /// assert!(WindowsVersion::from_str("7").is_some());
    /// assert!(WindowsVersion::from_str("invalid").is_none());
    /// ```
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "win11" | "windows11" | "11" => Some(Self::Win11),
            "win10" | "windows10" | "10" => Some(Self::Win10),
            "win81" | "windows81" | "8.1" => Some(Self::Win81),
            "win8" | "windows8" | "8" => Some(Self::Win8),
            "win7" | "windows7" | "7" => Some(Self::Win7),
            "vista" | "winvista" => Some(Self::Vista),
            "winxp" | "xp" => Some(Self::WinXP),
            _ => None,
        }
    }
}
