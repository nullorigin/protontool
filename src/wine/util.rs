use std::path::Path;
use std::process::Command;

pub fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let ext = archive.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    match ext.to_lowercase().as_str() {
        "zip" => extract_zip(archive, dest),
        "7z" => extract_7z(archive, dest),
        "tar" | "gz" | "tgz" | "bz2" | "xz" => extract_tar(archive, dest),
        "exe" => extract_exe(archive, dest),
        "cab" => extract_cab(archive, dest, None),
        "msi" => extract_msi(archive, dest),
        _ => Err(format!("Unsupported archive format: {}", ext)),
    }
}

pub fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(unzip) = crate::util::which("unzip") {
        let status = Command::new(unzip)
            .args(["-o", "-q", &archive.to_string_lossy(), "-d", &dest.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run unzip: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args(["x", "-y", &format!("-o{}", dest.to_string_lossy()), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("No zip extraction tool available (unzip or 7z required)".to_string())
}

pub fn extract_7z(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args(["x", "-y", &format!("-o{}", dest.to_string_lossy()), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("7z not available for extraction".to_string())
}

pub fn extract_tar(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(tar) = crate::util::which("tar") {
        let status = Command::new(tar)
            .args(["-xf", &archive.to_string_lossy(), "-C", &dest.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run tar: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("tar not available for extraction".to_string())
}

pub fn extract_exe(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args(["x", "-y", &format!("-o{}", dest.to_string_lossy()), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    if let Some(cabextract) = crate::util::which("cabextract") {
        let status = Command::new(cabextract)
            .args(["-d", &dest.to_string_lossy(), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run cabextract: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("No exe extraction tool available (7z or cabextract required)".to_string())
}

pub fn extract_cab(archive: &Path, dest: &Path, filter: Option<&str>) -> Result<(), String> {
    if let Some(cabextract) = crate::util::which("cabextract") {
        let mut args = vec!["-d".to_string(), dest.to_string_lossy().to_string()];
        
        if let Some(f) = filter {
            args.push("-F".to_string());
            args.push(f.to_string());
        }
        
        args.push(archive.to_string_lossy().to_string());
        
        let status = Command::new(cabextract)
            .args(&args)
            .status()
            .map_err(|e| format!("Failed to run cabextract: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("cabextract not available".to_string())
}

pub fn extract_msi(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(msiextract) = crate::util::which("msiextract") {
        let status = Command::new(msiextract)
            .args(["--directory", &dest.to_string_lossy(), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run msiextract: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args(["x", "-y", &format!("-o{}", dest.to_string_lossy()), &archive.to_string_lossy()])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;
        
        if status.success() {
            return Ok(());
        }
    }

    Err("No msi extraction tool available (msiextract or 7z required)".to_string())
}

pub fn copy_dll_to_system(dll_path: &Path, prefix_path: &Path, is_32bit: bool) -> Result<(), String> {
    let dest_dir = if is_32bit {
        prefix_path.join("drive_c/windows/syswow64")
    } else {
        prefix_path.join("drive_c/windows/system32")
    };

    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create system directory: {}", e))?;

    let filename = dll_path.file_name()
        .ok_or_else(|| "Invalid DLL path".to_string())?;
    
    let dest_path = dest_dir.join(filename);
    
    std::fs::copy(dll_path, &dest_path)
        .map_err(|e| format!("Failed to copy DLL: {}", e))?;

    Ok(())
}

pub fn create_symlink(target: &Path, link: &Path) -> Result<(), String> {
    if link.exists() {
        std::fs::remove_file(link).ok();
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|e| format!("Failed to create symlink: {}", e))?;
    }

    Ok(())
}

pub fn get_architecture(exe_path: &Path) -> Result<Architecture, String> {
    if let Some(file_cmd) = crate::util::which("file") {
        let output = Command::new(file_cmd)
            .arg(exe_path)
            .output()
            .map_err(|e| format!("Failed to run file command: {}", e))?;
        
        let output_str = String::from_utf8_lossy(&output.stdout);
        
        if output_str.contains("x86-64") || output_str.contains("x86_64") || output_str.contains("PE32+") {
            return Ok(Architecture::X64);
        } else if output_str.contains("80386") || output_str.contains("i386") || output_str.contains("PE32") {
            return Ok(Architecture::X86);
        }
    }

    Ok(Architecture::Unknown)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86,
    X64,
    Unknown,
}
