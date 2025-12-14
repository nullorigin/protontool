use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::steam::ProtonApp;

/// Wine prefix architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WineArch {
    Win32,
    Win64,
}

impl WineArch {
    pub fn as_str(&self) -> &'static str {
        match self {
            WineArch::Win32 => "win32",
            WineArch::Win64 => "win64",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "win32" | "32" | "x86" => Some(WineArch::Win32),
            "win64" | "64" | "x64" => Some(WineArch::Win64),
            _ => None,
        }
    }
}

impl Default for WineArch {
    fn default() -> Self {
        WineArch::Win64
    }
}

#[derive(Debug, Clone)]
pub struct WineContext {
    pub wine_path: PathBuf,
    pub wineserver_path: PathBuf,
    pub wine64_path: PathBuf,
    pub prefix_path: PathBuf,
    pub proton_path: PathBuf,
    pub arch: WineArch,
    pub dll_overrides: HashMap<String, String>,
    env: HashMap<String, String>,
}

impl WineContext {
    pub fn from_proton(proton_app: &ProtonApp, prefix_path: &Path) -> Self {
        Self::from_proton_with_arch(proton_app, prefix_path, WineArch::Win64)
    }

    pub fn from_proton_with_arch(proton_app: &ProtonApp, prefix_path: &Path, arch: WineArch) -> Self {
        let proton_dist = proton_app.install_path.join("dist");
        let proton_files = proton_app.install_path.join("files");
        
        let bin_dir = if proton_dist.exists() {
            proton_dist.join("bin")
        } else {
            proton_files.join("bin")
        };
        
        let lib_dir = if proton_dist.exists() {
            proton_dist.clone()
        } else {
            proton_files.clone()
        };

        let wine_path = bin_dir.join("wine");
        let wine64_path = bin_dir.join("wine64");
        let wineserver_path = bin_dir.join("wineserver");

        let mut env = HashMap::new();
        env.insert("WINE".to_string(), wine_path.to_string_lossy().to_string());
        env.insert("WINE64".to_string(), wine64_path.to_string_lossy().to_string());
        env.insert("WINESERVER".to_string(), wineserver_path.to_string_lossy().to_string());
        env.insert("WINEPREFIX".to_string(), prefix_path.to_string_lossy().to_string());
        env.insert("WINEDLLPATH".to_string(), format!(
            "{}:{}",
            lib_dir.join("lib64/wine").to_string_lossy(),
            lib_dir.join("lib/wine").to_string_lossy()
        ));
        env.insert("WINELOADER".to_string(), wine_path.to_string_lossy().to_string());
        env.insert("WINEARCH".to_string(), arch.as_str().to_string());

        Self {
            wine_path,
            wineserver_path,
            wine64_path,
            prefix_path: prefix_path.to_path_buf(),
            proton_path: proton_app.install_path.clone(),
            arch,
            dll_overrides: HashMap::new(),
            env,
        }
    }

    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    pub fn set_dll_override(&mut self, dll: &str, mode: &str) {
        self.dll_overrides.insert(dll.to_string(), mode.to_string());
    }

    fn build_dll_overrides_string(&self) -> String {
        self.dll_overrides.iter()
            .map(|(dll, mode)| format!("{}={}", dll, mode))
            .collect::<Vec<_>>()
            .join(";")
    }

    fn apply_env(&self, cmd: &mut Command) {
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
        
        if !self.dll_overrides.is_empty() {
            let overrides = self.build_dll_overrides_string();
            if let Ok(existing) = std::env::var("WINEDLLOVERRIDES") {
                cmd.env("WINEDLLOVERRIDES", format!("{};{}", existing, overrides));
            } else {
                cmd.env("WINEDLLOVERRIDES", overrides);
            }
        }
    }

    /// Run wine with the given arguments.
    /// By default, changes to the executable's directory if the first arg is a path to an executable.
    pub fn run_wine(&self, args: &[&str]) -> std::io::Result<Output> {
        self.run_wine_ex(args, None, true)
    }

    /// Run wine without changing the working directory.
    pub fn run_wine_no_cwd(&self, args: &[&str]) -> std::io::Result<Output> {
        self.run_wine_ex(args, None, false)
    }

    /// Run wine with explicit working directory.
    pub fn run_wine_cwd(&self, args: &[&str], cwd: &Path) -> std::io::Result<Output> {
        self.run_wine_ex(args, Some(cwd), true)
    }

    /// Run wine with full control over working directory behavior.
    /// - `cwd`: Explicit working directory, or None to auto-detect from first arg
    /// - `auto_cwd`: If true and cwd is None, change to executable's directory
    pub fn run_wine_ex(&self, args: &[&str], cwd: Option<&Path>, auto_cwd: bool) -> std::io::Result<Output> {
        let mut cmd = Command::new(&self.wine_path);
        cmd.args(args);
        
        // Determine working directory
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        } else if auto_cwd {
            // Auto-detect from first argument if it's an executable path
            if let Some(first_arg) = args.first() {
                let path = Path::new(first_arg);
                if path.is_absolute() || first_arg.contains('/') || first_arg.contains('\\') {
                    if let Some(parent) = path.parent() {
                        if parent.exists() {
                            cmd.current_dir(parent);
                        }
                    }
                }
            }
        }
        
        self.apply_env(&mut cmd);
        cmd.output()
    }

    pub fn run_wine64(&self, args: &[&str]) -> std::io::Result<Output> {
        let mut cmd = Command::new(&self.wine64_path);
        cmd.args(args);
        self.apply_env(&mut cmd);
        cmd.output()
    }

    pub fn run_wineboot(&self, init: bool) -> std::io::Result<Output> {
        let args = if init {
            vec!["wineboot", "--init"]
        } else {
            vec!["wineboot", "--update"]
        };
        self.run_wine(&args)
    }

    pub fn run_regedit(&self, reg_file: &Path) -> std::io::Result<Output> {
        self.run_wine(&["regedit", "/S", &reg_file.to_string_lossy()])
    }

    pub fn run_winecfg(&self, args: &[&str]) -> std::io::Result<Output> {
        let mut wine_args = vec!["winecfg"];
        wine_args.extend(args);
        self.run_wine(&wine_args)
    }

    pub fn run_regsvr32(&self, dll_path: &Path) -> std::io::Result<Output> {
        self.run_wine(&["regsvr32", "/s", &dll_path.to_string_lossy()])
    }

    pub fn run_msiexec(&self, msi_path: &Path, args: &[&str]) -> std::io::Result<Output> {
        let msi_str = msi_path.to_string_lossy().to_string();
        let mut wine_args_owned = vec!["msiexec".to_string(), "/i".to_string(), msi_str];
        for arg in args {
            wine_args_owned.push(arg.to_string());
        }
        let wine_args: Vec<&str> = wine_args_owned.iter().map(|s| s.as_str()).collect();
        self.run_wine(&wine_args)
    }

    pub fn run_executable(&self, exe_path: &Path, args: &[&str]) -> std::io::Result<Output> {
        let mut wine_args_owned = vec![exe_path.to_string_lossy().to_string()];
        wine_args_owned.extend(args.iter().map(|s| s.to_string()));
        let wine_args: Vec<&str> = wine_args_owned.iter().map(|s| s.as_str()).collect();
        self.run_wine(&wine_args)
    }

    pub fn wait_for_wineserver(&self) -> std::io::Result<Output> {
        Command::new(&self.wineserver_path)
            .arg("-w")
            .env("WINEPREFIX", &self.prefix_path)
            .output()
    }

    pub fn kill_wineserver(&self) -> std::io::Result<Output> {
        Command::new(&self.wineserver_path)
            .arg("-k")
            .env("WINEPREFIX", &self.prefix_path)
            .output()
    }

    pub fn get_system32_path(&self) -> PathBuf {
        self.prefix_path.join("drive_c/windows/system32")
    }

    pub fn get_syswow64_path(&self) -> PathBuf {
        self.prefix_path.join("drive_c/windows/syswow64")
    }

    pub fn get_program_files(&self) -> PathBuf {
        self.prefix_path.join("drive_c/Program Files")
    }

    pub fn get_program_files_x86(&self) -> PathBuf {
        self.prefix_path.join("drive_c/Program Files (x86)")
    }

    pub fn get_windows_path(&self) -> PathBuf {
        self.prefix_path.join("drive_c/windows")
    }

    pub fn get_fonts_path(&self) -> PathBuf {
        self.get_windows_path().join("Fonts")
    }
}
