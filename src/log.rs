use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::wine_data::KNOWN_ERRORS;

// Re-export wine_data items for convenience
pub use crate::wine_data::{WINE_DEBUG_CHANNELS, is_valid_channel, lookup_hresult, lookup_ntstatus, lookup_win32_error};

/// Maximum log file size before rotation (5 MB)
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

/// Number of rotated log files to keep
const MAX_LOG_FILES: usize = 5;

/// Log level for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Global logger instance
static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

/// Logger for protontool
pub struct Logger {
    log_dir: PathBuf,
    current_log: PathBuf,
    min_level: LogLevel,
}

impl Logger {
    /// Initialize the global logger
    pub fn init() -> Result<(), String> {
        let log_dir = crate::config::get_log_dir();
        fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log directory: {}", e))?;
        
        let current_log = log_dir.join("protontool.log");
        
        let logger = Logger {
            log_dir,
            current_log,
            min_level: LogLevel::Info,
        };
        
        // Rotate if needed
        logger.rotate_if_needed();
        
        let mut global = LOGGER.lock().unwrap();
        *global = Some(logger);
        
        Ok(())
    }
    
    /// Set the minimum log level
    pub fn set_level(level: LogLevel) {
        if let Ok(mut global) = LOGGER.lock() {
            if let Some(ref mut logger) = *global {
                logger.min_level = level;
            }
        }
    }
    
    /// Get current timestamp in ISO 8601 format
    fn timestamp() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        
        let secs = now.as_secs();
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        
        // Get date parts (approximate, good enough for logging)
        let days_since_epoch = secs / 86400;
        let mut year = 1970;
        let mut remaining_days = days_since_epoch;
        
        loop {
            let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }
        
        let days_in_months: [u64; 12] = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };
        
        let mut month = 1;
        for days in days_in_months {
            if remaining_days < days {
                break;
            }
            remaining_days -= days;
            month += 1;
        }
        let day = remaining_days + 1;
        
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hours, mins, s)
    }
    
    /// Rotate log files if the current one is too large
    fn rotate_if_needed(&self) {
        if let Ok(metadata) = fs::metadata(&self.current_log) {
            if metadata.len() >= MAX_LOG_SIZE {
                self.rotate();
            }
        }
    }
    
    /// Rotate log files
    fn rotate(&self) {
        // Remove oldest log if we have too many
        let oldest = self.log_dir.join(format!("protontool.{}.log", MAX_LOG_FILES));
        let _ = fs::remove_file(&oldest);
        
        // Shift existing logs
        for i in (1..MAX_LOG_FILES).rev() {
            let from = self.log_dir.join(format!("protontool.{}.log", i));
            let to = self.log_dir.join(format!("protontool.{}.log", i + 1));
            let _ = fs::rename(&from, &to);
        }
        
        // Move current to .1
        let first_backup = self.log_dir.join("protontool.1.log");
        let _ = fs::rename(&self.current_log, &first_backup);
    }
    
    /// Write a log message
    fn write(&self, level: LogLevel, message: &str) {
        if level < self.min_level {
            return;
        }
        
        self.rotate_if_needed();
        
        let timestamp = Self::timestamp();
        let formatted = format!("[{}] [{}] {}\n", timestamp, level, message);
        
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current_log)
        {
            let _ = file.write_all(formatted.as_bytes());
        }
        
        // Also print to stderr for errors/warnings
        match level {
            LogLevel::Error => eprint!("{}", formatted),
            LogLevel::Warning => eprint!("{}", formatted),
            _ => {}
        }
    }
}

/// Log a debug message
pub fn debug(message: &str) {
    if let Ok(global) = LOGGER.lock() {
        if let Some(ref logger) = *global {
            logger.write(LogLevel::Debug, message);
        }
    }
}

/// Log an info message
pub fn info(message: &str) {
    if let Ok(global) = LOGGER.lock() {
        if let Some(ref logger) = *global {
            logger.write(LogLevel::Info, message);
        }
    }
}

/// Log a warning message
pub fn warn(message: &str) {
    if let Ok(global) = LOGGER.lock() {
        if let Some(ref logger) = *global {
            logger.write(LogLevel::Warning, message);
        }
    }
}

/// Log an error message
pub fn error(message: &str) {
    if let Ok(global) = LOGGER.lock() {
        if let Some(ref logger) = *global {
            logger.write(LogLevel::Error, message);
        }
    }
}

/// Log executable output and scan for known errors
pub fn log_executable_output(executable: &str, stdout: &str, stderr: &str, exit_code: i32) {
    if let Ok(global) = LOGGER.lock() {
        if let Some(ref logger) = *global {
            // Log the execution
            logger.write(LogLevel::Info, &format!("Executed: {} (exit code: {})", executable, exit_code));
            
            // Log stdout if not empty
            if !stdout.trim().is_empty() {
                for line in stdout.lines() {
                    logger.write(LogLevel::Debug, &format!("[{}] stdout: {}", executable, line));
                }
            }
            
            // Log stderr if not empty
            if !stderr.trim().is_empty() {
                for line in stderr.lines() {
                    logger.write(LogLevel::Debug, &format!("[{}] stderr: {}", executable, line));
                }
            }
            
            // Scan for known errors and print formatted output
            let combined = format!("{}\n{}", stdout, stderr);
            let matches = scan_for_errors(&combined);
            
            if !matches.is_empty() {
                println!();
                for (code, description) in matches {
                    let formatted = format_error_message(executable, &code, &description);
                    print!("{}", formatted);
                    logger.write(LogLevel::Warning, &format!("[{}] Known issue detected: {} - {}", executable, code, description));
                }
            }
            
            // Log non-zero exit code as error
            if exit_code != 0 {
                logger.write(LogLevel::Error, &format!("[{}] Exited with code {}", executable, exit_code));
            }
        }
    }
}

/// Format an error message nicely
fn format_error_message(executable: &str, code: &str, description: &str) -> String {
    format!(
        "┌─ {} ─────────────────────────────────────────\n\
         │ Code: {}\n\
         │ Details: {}\n\
         └────────────────────────────────────────────────────\n",
        executable, code, description
    )
}

/// Extract DLL name from a line of Wine output
fn extract_dll_name(line: &str) -> Option<String> {
    // Common patterns in Wine output:
    // "Library MSVCP140.dll (which is needed by ...)"
    // "failed to load L\"d3d11.dll\""
    // "could not load \"vcruntime140.dll\""
    // "Module not found: msvcr120.dll"
    
    let line_lower = line.to_lowercase();
    
    // Find .dll in the line
    if let Some(dll_pos) = line_lower.find(".dll") {
        // Walk backwards to find start of DLL name
        let before_dll = &line[..dll_pos];
        let start = before_dll.rfind(|c: char| {
            !c.is_alphanumeric() && c != '_' && c != '-'
        }).map(|i| i + 1).unwrap_or(0);
        
        let dll_name = &line[start..dll_pos + 4]; // +4 for ".dll"
        
        // Clean up the name (remove quotes, backslashes, etc.)
        let cleaned = dll_name
            .trim_start_matches(|c: char| c == '"' || c == '\'' || c == 'L' || c == '\\')
            .trim_end_matches(|c: char| c == '"' || c == '\'');
        
        if !cleaned.is_empty() && cleaned.len() > 4 {
            return Some(cleaned.to_string());
        }
    }
    
    None
}

/// Scan output for known Wine/Windows error codes
fn scan_for_errors(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let output_lower = output.to_lowercase();
    let lines: Vec<&str> = output.lines().collect();
    
    for (pattern, code, description) in KNOWN_ERRORS.iter() {
        let pattern_lower = pattern.to_lowercase();
        if output_lower.contains(&pattern_lower) {
            // Check if this is a DLL-related error
            let is_dll_error = code.contains("NODLL") 
                || code.contains("MODULE") 
                || code.contains("DLL")
                || code.contains("ORDINAL")
                || code.contains("ENTRYPT");
            
            if is_dll_error {
                // Find the line(s) containing this pattern and extract DLL names
                let mut dll_names: Vec<String> = Vec::new();
                
                for (i, line) in lines.iter().enumerate() {
                    if line.to_lowercase().contains(&pattern_lower) {
                        // Check this line and nearby lines for DLL names
                        for offset in 0..=2 {
                            if i + offset < lines.len() {
                                if let Some(dll) = extract_dll_name(lines[i + offset]) {
                                    if !dll_names.contains(&dll) {
                                        dll_names.push(dll);
                                    }
                                }
                            }
                        }
                    }
                }
                
                if !dll_names.is_empty() {
                    let dll_list = dll_names.join(", ");
                    let enhanced_desc = format!("{} [Missing: {}]", description, dll_list);
                    found.push((code.to_string(), enhanced_desc));
                } else {
                    found.push((code.to_string(), description.to_string()));
                }
            } else {
                found.push((code.to_string(), description.to_string()));
            }
        }
    }
    
    found
}


/// Get the path to the current log file
pub fn get_current_log_path() -> PathBuf {
    crate::config::get_log_dir().join("protontool.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dll_name() {
        // Wine import_dll pattern
        assert_eq!(
            extract_dll_name("err:module:import_dll Library MSVCP140.dll (which is needed by L\"game.exe\")"),
            Some("MSVCP140.dll".to_string())
        );
        
        // Wine load_dll pattern with L prefix
        assert_eq!(
            extract_dll_name("err:module:load_dll failed to load L\"d3d11.dll\""),
            Some("d3d11.dll".to_string())
        );
        
        // Quoted DLL name
        assert_eq!(
            extract_dll_name("could not load \"vcruntime140.dll\""),
            Some("vcruntime140.dll".to_string())
        );
        
        // DLL in path
        assert_eq!(
            extract_dll_name("Module not found: C:\\windows\\system32\\msvcr120.dll"),
            Some("msvcr120.dll".to_string())
        );
        
        // No DLL in line
        assert_eq!(extract_dll_name("some random error message"), None);
    }

    #[test]
    fn test_scan_for_errors_with_dll() {
        let output = "err:module:import_dll Library MSVCP140.dll (which is needed by L\"game.exe\") not found";
        let errors = scan_for_errors(output);
        
        assert!(!errors.is_empty());
        // Should contain the DLL name in the description
        assert!(errors.iter().any(|(_, desc)| desc.contains("MSVCP140.dll")));
    }
}

/// Read the last N lines from the current log
pub fn tail_log(lines: usize) -> Vec<String> {
    let log_path = get_current_log_path();
    
    if let Ok(file) = File::open(&log_path) {
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
        
        if all_lines.len() > lines {
            all_lines[all_lines.len() - lines..].to_vec()
        } else {
            all_lines
        }
    } else {
        Vec::new()
    }
}

/// Parsed log entry for the viewer
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub count: usize,
}

/// Parse log file and deduplicate entries, returning aggregated entries
pub fn parse_log_deduplicated(
    show_error: bool,
    show_warning: bool,
    show_info: bool,
    show_debug: bool,
    search_filter: Option<&str>,
) -> Vec<LogEntry> {
    let log_path = get_current_log_path();
    let mut entries: std::collections::HashMap<(String, String), LogEntry> = std::collections::HashMap::new();
    
    let file = match File::open(&log_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    
    let reader = BufReader::new(file);
    
    for line in reader.lines().filter_map(|l| l.ok()) {
        // Parse line format: [TIMESTAMP] [LEVEL] message
        // Example: [2024-01-15 10:30:45] [INFO] Some message
        let parts: Vec<&str> = line.splitn(3, "] ").collect();
        if parts.len() < 2 {
            continue;
        }
        
        let timestamp = parts[0].trim_start_matches('[').to_string();
        let level_part = parts.get(1).unwrap_or(&"");
        let level = level_part.trim_start_matches('[').trim_end_matches(']').to_string();
        let message = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
        
        // Filter by level
        let include = match level.as_str() {
            "ERROR" => show_error,
            "WARN" => show_warning,
            "INFO" => show_info,
            "DEBUG" => show_debug,
            _ => show_info, // Default to info for unknown levels
        };
        
        if !include {
            continue;
        }
        
        // Filter by search term
        if let Some(filter) = search_filter {
            let filter_lower = filter.to_lowercase();
            if !message.to_lowercase().contains(&filter_lower) 
                && !level.to_lowercase().contains(&filter_lower) {
                continue;
            }
        }
        
        // Deduplicate by (level, message)
        let key = (level.clone(), message.clone());
        if let Some(entry) = entries.get_mut(&key) {
            entry.count += 1;
            entry.timestamp = timestamp; // Update to latest timestamp
        } else {
            entries.insert(key, LogEntry {
                timestamp,
                level,
                message,
                count: 1,
            });
        }
    }
    
    // Convert to vec and sort by timestamp (most recent first)
    let mut result: Vec<LogEntry> = entries.into_values().collect();
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    result
}
