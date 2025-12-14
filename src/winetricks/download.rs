use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Downloader {
    cache_dir: PathBuf,
}

impl Downloader {
    pub fn new(cache_dir: &Path) -> Self {
        fs::create_dir_all(cache_dir).ok();
        Self {
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn download(&self, url: &str, filename: &str, expected_sha256: Option<&str>) -> Result<PathBuf, String> {
        let cached_path = self.cache_dir.join(filename);
        
        if cached_path.exists() {
            if let Some(expected) = expected_sha256 {
                if self.verify_sha256(&cached_path, expected)? {
                    return Ok(cached_path);
                }
                fs::remove_file(&cached_path).ok();
            } else {
                return Ok(cached_path);
            }
        }

        self.download_file(url, &cached_path)?;

        if let Some(expected) = expected_sha256 {
            if !self.verify_sha256(&cached_path, expected)? {
                fs::remove_file(&cached_path).ok();
                return Err(format!("SHA256 verification failed for {}", filename));
            }
        }

        Ok(cached_path)
    }

    fn download_file(&self, url: &str, dest: &Path) -> Result<(), String> {
        if let Some(curl) = crate::util::which("curl") {
            let status = Command::new(curl)
                .args(["-L", "-o", &dest.to_string_lossy(), "--progress-bar", url])
                .status()
                .map_err(|e| format!("Failed to run curl: {}", e))?;
            
            if status.success() {
                return Ok(());
            }
        }

        if let Some(wget) = crate::util::which("wget") {
            let status = Command::new(wget)
                .args(["-O", &dest.to_string_lossy(), "--progress=bar", url])
                .status()
                .map_err(|e| format!("Failed to run wget: {}", e))?;
            
            if status.success() {
                return Ok(());
            }
        }

        Err("No download tool available (curl or wget required)".to_string())
    }

    fn verify_sha256(&self, path: &Path, expected: &str) -> Result<bool, String> {
        if let Some(sha256sum) = crate::util::which("sha256sum") {
            let output = Command::new(sha256sum)
                .arg(path)
                .output()
                .map_err(|e| format!("Failed to run sha256sum: {}", e))?;
            
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let computed = output_str.split_whitespace().next().unwrap_or("");
                return Ok(computed.eq_ignore_ascii_case(expected));
            }
        }

        if let Some(openssl) = crate::util::which("openssl") {
            let output = Command::new(openssl)
                .args(["dgst", "-sha256", &path.to_string_lossy()])
                .output()
                .map_err(|e| format!("Failed to run openssl: {}", e))?;
            
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let computed = output_str.split('=').last()
                    .map(|s| s.trim())
                    .unwrap_or("");
                return Ok(computed.eq_ignore_ascii_case(expected));
            }
        }

        Ok(true)
    }

    pub fn get_cached_path(&self, filename: &str) -> PathBuf {
        self.cache_dir.join(filename)
    }

    pub fn is_cached(&self, filename: &str) -> bool {
        self.cache_dir.join(filename).exists()
    }

    pub fn clear_cache(&self) -> Result<(), String> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)
                .map_err(|e| format!("Failed to clear cache: {}", e))?;
            fs::create_dir_all(&self.cache_dir)
                .map_err(|e| format!("Failed to recreate cache directory: {}", e))?;
        }
        Ok(())
    }
}
