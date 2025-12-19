//! Wine prefix initialization module
//! 
//! Creates Wine prefixes by copying from Proton's default_pfx and running wineboot.
//! This approach ensures proper DLL structure and avoids cross-filesystem issues.

use std::fs;
use std::path::Path;

use crate::wine::registry::{filter_registry_file, FILTER_REGISTRY_KEYS};

/// Recursively copy a directory, resolving symlinks to copy actual file contents
/// Skips the dosdevices directory (created separately)
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let filename = entry.file_name();
        
        // Skip dosdevices - we'll create it manually
        if filename == "dosdevices" {
            continue;
        }
        
        let file_type = entry.file_type()?;
        
        if file_type.is_symlink() {
            // Resolve symlink and copy the actual file/directory
            let target = fs::read_link(&src_path)?;
            let resolved = if target.is_absolute() {
                target
            } else {
                src_path.parent().unwrap_or(src).join(&target)
            };
            
            if resolved.is_dir() {
                copy_dir_recursive(&resolved, &dst_path)?;
            } else if resolved.is_file() {
                fs::copy(&resolved, &dst_path)?;
            }
            // Skip broken symlinks
        } else if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    
    Ok(())
}

/// Create the dosdevices directory with required drive symlinks
#[cfg(unix)]
fn create_dosdevices(prefix_dir: &Path) -> std::io::Result<()> {
    let dosdevices = prefix_dir.join("dosdevices");
    fs::create_dir_all(&dosdevices)?;
    
    // c: -> ../drive_c
    let c_link = dosdevices.join("c:");
    if !c_link.exists() {
        std::os::unix::fs::symlink("../drive_c", &c_link)?;
    }
    
    // z: -> /
    let z_link = dosdevices.join("z:");
    if !z_link.exists() {
        std::os::unix::fs::symlink("/", &z_link)?;
    }
    
    Ok(())
}

#[cfg(not(unix))]
fn create_dosdevices(prefix_dir: &Path) -> std::io::Result<()> {
    let dosdevices = prefix_dir.join("dosdevices");
    fs::create_dir_all(&dosdevices)?;
    // On non-unix, wineboot will create the symlinks
    Ok(())
}

/// Initialize a Wine prefix using the Proton method
/// 
/// This copies from Proton's default_pfx (which has correct DLL structure)
/// and then runs wineboot to complete initialization.
/// 
/// # Arguments
/// * `prefix_dir` - Path to the Wine prefix to initialize
/// * `dist_dir` - Path to the Proton/Wine distribution directory (files/ or dist/)
/// * `wine_ctx` - Optional WineContext for running wineboot (if None, skips wineboot)
pub fn init_prefix(
    prefix_dir: &Path,
    dist_dir: &Path,
    run_wineboot: bool,
    wine_ctx: Option<&crate::wine::WineContext>,
) -> std::io::Result<()> {
    // Check for default_pfx in Proton's share directory
    let default_pfx = dist_dir.join("share/default_pfx");
    
    if default_pfx.exists() {
        eprintln!("Copying from Proton's default prefix...");
        // Copy default_pfx to prefix_dir (skips dosdevices)
        copy_dir_recursive(&default_pfx, prefix_dir)?;
    } else {
        // Fallback: create directory and let wineboot create from scratch
        eprintln!("No default_pfx found, creating fresh prefix...");
        fs::create_dir_all(prefix_dir)?;
    }
    
    // Create dosdevices with proper symlinks
    eprintln!("Creating drive links...");
    create_dosdevices(prefix_dir)?;
    
    // Run wineboot to complete/update the prefix
    if run_wineboot {
        if let Some(ctx) = wine_ctx {
            eprintln!("Running wineboot to initialize prefix...");
            match ctx.run_wine_no_cwd(&["wineboot", "--init"]) {
                Ok(output) => {
                    if !output.status.success() {
                        eprintln!("Warning: wineboot returned non-zero exit code");
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to run wineboot: {}", e);
                }
            }
            
            // Wait for wineserver to finish
            let _ = ctx.wait_for_wineserver();
        } else {
            eprintln!("No wine context provided, skipping wineboot");
        }
    }
    
    // Filter registry files
    eprintln!("Filtering registry files...");
    let user_reg = prefix_dir.join("user.reg");
    let system_reg = prefix_dir.join("system.reg");
    
    if user_reg.exists() {
        if let Err(e) = filter_registry_file(&user_reg, FILTER_REGISTRY_KEYS) {
            eprintln!("Warning: Failed to filter user.reg: {}", e);
        }
    }
    
    if system_reg.exists() {
        if let Err(e) = filter_registry_file(&system_reg, FILTER_REGISTRY_KEYS) {
            eprintln!("Warning: Failed to filter system.reg: {}", e);
        }
    }
    
    eprintln!("Prefix initialization complete.");
    Ok(())
}

