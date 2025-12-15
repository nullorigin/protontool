use std::path::Path;

use super::wine::WineContext;

pub struct RegistryEditor<'a> {
    wine_ctx: &'a WineContext,
}

impl<'a> RegistryEditor<'a> {
    pub fn new(wine_ctx: &'a WineContext) -> Self {
        Self { wine_ctx }
    }

    pub fn set_value(&self, key: &str, name: &str, value: &str, value_type: RegType) -> Result<(), String> {
        let reg_content = format!(
            "Windows Registry Editor Version 5.00\n\n[{}]\n\"{}\"={}",
            key,
            name,
            value_type.format_value(value)
        );
        
        self.apply_reg_content(&reg_content)
    }

    pub fn delete_value(&self, key: &str, name: &str) -> Result<(), String> {
        let reg_content = format!(
            "Windows Registry Editor Version 5.00\n\n[{}]\n\"{}\"=-",
            key,
            name
        );
        
        self.apply_reg_content(&reg_content)
    }

    pub fn delete_key(&self, key: &str) -> Result<(), String> {
        let reg_content = format!(
            "Windows Registry Editor Version 5.00\n\n[-{}]",
            key
        );
        
        self.apply_reg_content(&reg_content)
    }

    pub fn apply_reg_file(&self, reg_file: &Path) -> Result<(), String> {
        self.wine_ctx.run_regedit(reg_file)
            .map_err(|e| format!("Failed to apply registry file: {}", e))?;
        Ok(())
    }

    fn apply_reg_content(&self, content: &str) -> Result<(), String> {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("protontool_reg_patch.reg");
        
        std::fs::write(&temp_file, content)
            .map_err(|e| format!("Failed to write temp registry file: {}", e))?;
        
        let result = self.wine_ctx.run_regedit(&temp_file)
            .map_err(|e| format!("Failed to apply registry patch: {}", e));
        
        std::fs::remove_file(&temp_file).ok();
        
        result?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RegType {
    String,
    Dword,
    Binary,
    ExpandString,
    MultiString,
}

impl RegType {
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
                let hex_bytes: Vec<String> = value.as_bytes()
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

    fn string_to_utf16_hex(s: &str) -> String {
        let utf16: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        utf16.iter()
            .flat_map(|&c| vec![(c & 0xFF) as u8, (c >> 8) as u8])
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(",")
    }
}

pub fn set_windows_version(wine_ctx: &WineContext, version: WindowsVersion) -> Result<(), String> {
    let editor = RegistryEditor::new(wine_ctx);
    
    let (product_name, csd_version, build, build_num, current_version, csd_dword) = match version {
        WindowsVersion::Win11 => ("Microsoft Windows 11", "", "22000", "22000", "6.3", 0u32),
        WindowsVersion::Win10 => ("Microsoft Windows 10", "", "19041", "19041", "6.3", 0),
        WindowsVersion::Win81 => ("Microsoft Windows 8.1", "", "9600", "9600", "6.3", 0),
        WindowsVersion::Win8 => ("Microsoft Windows 8", "", "9200", "9200", "6.2", 0),
        WindowsVersion::Win7 => ("Microsoft Windows 7", "Service Pack 1", "7601", "7601", "6.1", 0x100),
        WindowsVersion::Vista => ("Microsoft Windows Vista", "Service Pack 2", "6002", "6002", "6.0", 0x200),
        WindowsVersion::WinXP => ("Microsoft Windows XP", "Service Pack 3", "2600", "2600", "5.1", 0x300),
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
