//! Archive extraction and file utilities for Wine verbs.

use std::path::Path;
use std::process::Command;

/// Extract an archive to a destination directory.
/// Automatically detects format from extension and uses appropriate tool.
pub fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let ext = archive.extension().and_then(|e| e.to_str()).unwrap_or("");

    let filename = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Check if it's a tar archive with compression
    let is_tar_compressed = filename.contains(".tar.");

    match (ext.to_lowercase().as_str(), is_tar_compressed) {
        ("zip", _) => extract_zip(archive, dest),
        ("7z", _) => extract_7z(archive, dest),
        ("tar" | "tgz" | "tbz2" | "txz" | "tlz", _) => extract_tar(archive, dest),
        ("gz" | "bz2" | "xz" | "lz", true) => extract_tar(archive, dest),
        ("zst", true) => extract_zst(archive, dest),
        ("gz", false) => extract_gzip(archive, dest),
        ("bz2", false) => extract_bzip2(archive, dest),
        ("xz", false) => extract_xz(archive, dest),
        ("lz", false) => extract_lzip(archive, dest),
        ("zst", false) => extract_zst(archive, dest),
        ("exe", _) => extract_exe(archive, dest),
        ("cab", _) => extract_cab(archive, dest, None),
        ("msi", _) => extract_msi(archive, dest),
        _ => Err(format!("Unsupported archive format: {}", ext)),
    }
}

/// Extract a ZIP archive using unzip or 7z.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(unzip) = crate::util::which("unzip") {
        let status = Command::new(unzip)
            .args([
                "-o",
                "-q",
                &archive.to_string_lossy(),
                "-d",
                &dest.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run unzip: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args([
                "x",
                "-y",
                &format!("-o{}", dest.to_string_lossy()),
                &archive.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No zip extraction tool available (unzip or 7z required)".to_string())
}

/// Extract a 7z archive using 7z.
pub fn extract_7z(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args([
                "x",
                "-y",
                &format!("-o{}", dest.to_string_lossy()),
                &archive.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("7z not available for extraction".to_string())
}

/// Extract a tar archive (handles .tar, .tar.gz, .tar.bz2, .tar.xz).
pub fn extract_tar(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(tar) = crate::util::which("tar") {
        let status = Command::new(tar)
            .args([
                "-xf",
                &archive.to_string_lossy(),
                "-C",
                &dest.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run tar: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("tar not available for extraction".to_string())
}

/// Extract a zstd-compressed file or .tar.zst archive.
pub fn extract_zst(archive: &Path, dest: &Path) -> Result<(), String> {
    let filename = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Check if it's a .tar.zst file
    let is_tar_zst = filename.ends_with(".tar.zst");

    if is_tar_zst {
        // Try tar with --zstd flag first (modern tar supports this)
        if let Some(tar) = crate::util::which("tar") {
            let status = Command::new(&tar)
                .args([
                    "--zstd",
                    "-xf",
                    &archive.to_string_lossy(),
                    "-C",
                    &dest.to_string_lossy(),
                ])
                .status()
                .map_err(|e| format!("Failed to run tar: {}", e))?;

            if status.success() {
                return Ok(());
            }

            // Fallback: pipe zstd to tar
            if let Some(zstd) = crate::util::which("zstd") {
                let zstd_proc = Command::new(&zstd)
                    .args(["-d", "-c", &archive.to_string_lossy()])
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to run zstd: {}", e))?;

                let status = Command::new(&tar)
                    .args(["-xf", "-", "-C", &dest.to_string_lossy()])
                    .stdin(zstd_proc.stdout.unwrap())
                    .status()
                    .map_err(|e| format!("Failed to run tar: {}", e))?;

                if status.success() {
                    return Ok(());
                }
            }
        }
    } else {
        // Plain .zst file - decompress to destination
        if let Some(zstd) = crate::util::which("zstd") {
            let output_name = archive
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("output");
            let output_path = dest.join(output_name);

            let status = Command::new(zstd)
                .args([
                    "-d",
                    &archive.to_string_lossy(),
                    "-o",
                    &output_path.to_string_lossy(),
                ])
                .status()
                .map_err(|e| format!("Failed to run zstd: {}", e))?;

            if status.success() {
                return Ok(());
            }
        }
    }

    Err("No zstd extraction tool available (zstd required, or tar with zstd support)".to_string())
}

/// Extract a standalone .gz file (not tar.gz).
pub fn extract_gzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let output_name = archive
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let output_path = dest.join(output_name);

    // Try gzip
    if let Some(gzip) = crate::util::which("gzip") {
        let status = Command::new(gzip)
            .args(["-d", "-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run gzip: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    // Fallback to gunzip
    if let Some(gunzip) = crate::util::which("gunzip") {
        let status = Command::new(gunzip)
            .args(["-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run gunzip: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No gzip extraction tool available (gzip or gunzip required)".to_string())
}

/// Extract a standalone .bz2 file (not tar.bz2).
pub fn extract_bzip2(archive: &Path, dest: &Path) -> Result<(), String> {
    let output_name = archive
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let output_path = dest.join(output_name);

    // Try bzip2
    if let Some(bzip2) = crate::util::which("bzip2") {
        let status = Command::new(bzip2)
            .args(["-d", "-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run bzip2: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    // Fallback to bunzip2
    if let Some(bunzip2) = crate::util::which("bunzip2") {
        let status = Command::new(bunzip2)
            .args(["-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run bunzip2: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No bzip2 extraction tool available (bzip2 or bunzip2 required)".to_string())
}

/// Extract a standalone .xz file (not tar.xz).
pub fn extract_xz(archive: &Path, dest: &Path) -> Result<(), String> {
    let output_name = archive
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let output_path = dest.join(output_name);

    // Try xz
    if let Some(xz) = crate::util::which("xz") {
        let status = Command::new(xz)
            .args(["-d", "-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run xz: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    // Fallback to unxz
    if let Some(unxz) = crate::util::which("unxz") {
        let status = Command::new(unxz)
            .args(["-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run unxz: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No xz extraction tool available (xz or unxz required)".to_string())
}

/// Extract a standalone .lz file (not tar.lz).
pub fn extract_lzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let output_name = archive
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let output_path = dest.join(output_name);

    // Try lzip
    if let Some(lzip) = crate::util::which("lzip") {
        let status = Command::new(lzip)
            .args(["-d", "-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run lzip: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    // Fallback to lunzip
    if let Some(lunzip) = crate::util::which("lunzip") {
        let status = Command::new(lunzip)
            .args(["-c", &archive.to_string_lossy()])
            .stdout(
                std::fs::File::create(&output_path)
                    .map_err(|e| format!("Failed to create output file: {}", e))?,
            )
            .status()
            .map_err(|e| format!("Failed to run lunzip: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No lzip extraction tool available (lzip or lunzip required)".to_string())
}

/// Extract an EXE (self-extracting archive) using 7z or cabextract.
pub fn extract_exe(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args([
                "x",
                "-y",
                &format!("-o{}", dest.to_string_lossy()),
                &archive.to_string_lossy(),
            ])
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

/// Extract a CAB archive using cabextract.
/// Optional filter parameter extracts only matching files.
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

/// Extract an MSI installer using msiextract or 7z.
pub fn extract_msi(archive: &Path, dest: &Path) -> Result<(), String> {
    if let Some(msiextract) = crate::util::which("msiextract") {
        let status = Command::new(msiextract)
            .args([
                "--directory",
                &dest.to_string_lossy(),
                &archive.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run msiextract: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    if let Some(p7zip) = crate::util::which("7z") {
        let status = Command::new(p7zip)
            .args([
                "x",
                "-y",
                &format!("-o{}", dest.to_string_lossy()),
                &archive.to_string_lossy(),
            ])
            .status()
            .map_err(|e| format!("Failed to run 7z: {}", e))?;

        if status.success() {
            return Ok(());
        }
    }

    Err("No msi extraction tool available (msiextract or 7z required)".to_string())
}

/// Copy a DLL to the appropriate system directory in the Wine prefix.
/// 32-bit DLLs go to syswow64, 64-bit to system32.
pub fn copy_dll_to_system(
    dll_path: &Path,
    prefix_path: &Path,
    is_32bit: bool,
) -> Result<(), String> {
    let dest_dir = if is_32bit {
        prefix_path.join("drive_c/windows/syswow64")
    } else {
        prefix_path.join("drive_c/windows/system32")
    };

    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create system directory: {}", e))?;

    let filename = dll_path
        .file_name()
        .ok_or_else(|| "Invalid DLL path".to_string())?;

    let dest_path = dest_dir.join(filename);

    std::fs::copy(dll_path, &dest_path).map_err(|e| format!("Failed to copy DLL: {}", e))?;

    Ok(())
}

/// Detect the architecture (x86/x64) of a PE executable using `file` command.
pub fn get_architecture(exe_path: &Path) -> Result<Architecture, String> {
    if let Some(file_cmd) = crate::util::which("file") {
        let output = Command::new(file_cmd)
            .arg(exe_path)
            .output()
            .map_err(|e| format!("Failed to run file command: {}", e))?;

        let output_str = String::from_utf8_lossy(&output.stdout);

        if output_str.contains("x86-64")
            || output_str.contains("x86_64")
            || output_str.contains("PE32+")
        {
            return Ok(Architecture::X64);
        } else if output_str.contains("80386")
            || output_str.contains("i386")
            || output_str.contains("PE32")
        {
            return Ok(Architecture::X86);
        }
    }

    Ok(Architecture::Unknown)
}

/// CPU architecture for PE executables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86,
    X64,
    Unknown,
}
