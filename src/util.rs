use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::os::unix::fs::symlink;

/// Extract stdout from a command output as a trimmed string
pub fn output_to_string(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Extract stderr from a command output as a trimmed string  
pub fn output_stderr_to_string(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
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

/// Recursively walk directory and collect files with given extension
pub fn walk_dir_files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_dir_files_with_ext_recursive(dir, ext, &mut files);
    files
}

fn walk_dir_files_with_ext_recursive(dir: &Path, ext: &str, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // Use symlink_metadata to avoid following symlinks
            let metadata = match path.symlink_metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            // Skip symlinks to avoid infinite loops
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                walk_dir_files_with_ext_recursive(&path, ext, files);
            } else if metadata.is_file() && path.extension().map_or(false, |e| e == ext) {
                files.push(path);
            }
        }
    }
}

/// Parse hex value from string like "0x12345678" or "0X12345678"
pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.len() > 2 && (s.starts_with("0x") || s.starts_with("0X")) {
        u32::from_str_radix(&s[2..], 16).ok()
    } else {
        None
    }
}

/// Calculate relative path from base directory to target
pub fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = from.canonicalize().ok()?;
    let to = to.canonicalize().ok()?;
    
    let from_parts: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    
    // Find common prefix length
    let common_len = from_parts.iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    
    // Build relative path
    let mut result = PathBuf::new();
    
    // Add ".." for each remaining component in from
    for _ in common_len..from_parts.len() {
        result.push("..");
    }
    
    // Add remaining components from to
    for part in &to_parts[common_len..] {
        result.push(part);
    }
    
    Some(result)
}
pub fn make_symlink(target: &Path, link: &Path, relative: bool) -> std::io::Result<()> {
    if link.exists() {
        std::fs::remove_file(link).ok();
    }

    #[cfg(unix)]
    {
        if relative {
            let target = target.canonicalize()?;
            let linkname_abs = if link.is_absolute() {
                link.to_path_buf()
            } else {
                env::current_dir()?.join(link)
            };
            
            let link_dir = linkname_abs.parent().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid link path")
            })?;
            
            // Calculate relative path from link directory to target
            let rel_path = relative_path(link_dir, &target).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Cannot compute relative path")
            })?;
            
            symlink(&rel_path, &linkname_abs)?;
        } else {
            symlink(target, link)?;
        }
    }

    Ok(())
}

/// Create a relative symlink from linkname pointing to target
pub fn make_relative_symlink(target: &Path, linkname: &Path) -> std::io::Result<()> {
    make_symlink(target, linkname, true)
}

