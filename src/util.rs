use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn is_steam_deck() -> bool {
    if let Ok(board) = fs::read_to_string("/sys/class/dmi/id/board_name") {
        return board.trim() == "Jupiter";
    }
    false
}

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

pub fn run_command(
    _winetricks_path: &Path,
    proton_app: &crate::steam::ProtonApp,
    steam_app: &crate::steam::SteamApp,
    use_steam_runtime: bool,
    legacy_steam_runtime_path: Option<&Path>,
    command: &[String],
    _use_bwrap: Option<bool>,
    _start_wineserver: bool,
    cwd: Option<&str>,
    shell: bool,
) -> i32 {
    let proton_dist = proton_app.install_path.join("dist");
    let proton_files = proton_app.install_path.join("files");
    
    let wine_path = if proton_dist.exists() {
        proton_dist.join("bin/wine")
    } else {
        proton_files.join("bin/wine")
    };
    
    let wineserver_path = wine_path.parent().unwrap().join("wineserver");
    
    let prefix_path = steam_app.prefix_path.as_ref()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    
    let mut cmd_env: Vec<(String, String)> = Vec::new();
    
    cmd_env.push(("WINE".to_string(), wine_path.to_string_lossy().to_string()));
    cmd_env.push(("WINESERVER".to_string(), wineserver_path.to_string_lossy().to_string()));
    cmd_env.push(("WINEPREFIX".to_string(), prefix_path.to_string_lossy().to_string()));
    cmd_env.push(("WINEDLLPATH".to_string(), format!(
        "{}:{}",
        wine_path.parent().unwrap().parent().unwrap().join("lib64/wine").to_string_lossy(),
        wine_path.parent().unwrap().parent().unwrap().join("lib/wine").to_string_lossy()
    )));
    
    cmd_env.push(("STEAM_APPID".to_string(), steam_app.appid.to_string()));
    cmd_env.push(("STEAM_APP_PATH".to_string(), steam_app.install_path.to_string_lossy().to_string()));
    cmd_env.push(("PROTON_PATH".to_string(), proton_app.install_path.to_string_lossy().to_string()));
    
    if use_steam_runtime {
        if let Some(runtime_path) = legacy_steam_runtime_path {
            let ld_path = format!(
                "{}:{}",
                runtime_path.join("i386/lib/i386-linux-gnu").to_string_lossy(),
                runtime_path.join("amd64/lib/x86_64-linux-gnu").to_string_lossy()
            );
            if let Ok(existing) = env::var("LD_LIBRARY_PATH") {
                cmd_env.push(("LD_LIBRARY_PATH".to_string(), format!("{}:{}", ld_path, existing)));
            } else {
                cmd_env.push(("LD_LIBRARY_PATH".to_string(), ld_path));
            }
        }
    }
    
    let working_dir = cwd.map(|s| s.to_string())
        .unwrap_or_else(|| env::current_dir().unwrap().to_string_lossy().to_string());
    
    let status = if shell {
        let shell_cmd = command.join(" ");
        let mut process = Command::new("sh");
        process.arg("-c").arg(&shell_cmd);
        process.current_dir(&working_dir);
        for (key, val) in &cmd_env {
            process.env(key, val);
        }
        process.status()
    } else {
        if command.is_empty() {
            return 1;
        }
        let mut process = Command::new(&command[0]);
        if command.len() > 1 {
            process.args(&command[1..]);
        }
        process.current_dir(&working_dir);
        for (key, val) in &cmd_env {
            process.env(key, val);
        }
        process.status()
    };
    
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 1,
    }
}

pub fn which(name: &str) -> Option<std::path::PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|dir| {
            let full_path = dir.join(name);
            if full_path.is_file() && is_executable(&full_path) {
                Some(full_path)
            } else {
                None
            }
        })
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = path.metadata() {
        let permissions = metadata.permissions();
        permissions.mode() & 0o111 != 0
    } else {
        false
    }
}

pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    if s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/') {
        return s.to_string();
    }

    let mut quoted = String::with_capacity(s.len() + 2);
    quoted.push('\'');
    for c in s.chars() {
        if c == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}
