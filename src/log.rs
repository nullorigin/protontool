use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::wine_data::KNOWN_ERRORS;

// Re-export wine_data items for convenience
pub use crate::wine_data::{WINE_DEBUG_CHANNELS, is_valid_channel, lookup_error};

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

/// Scan output for known Wine/Windows error codes
fn scan_for_errors(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let output_lower = output.to_lowercase();
    
    for (pattern, code, description) in KNOWN_ERRORS.iter() {
        if output_lower.contains(&pattern.to_lowercase()) {
            found.push((code.to_string(), description.to_string()));
        }
    }
    
    found
}


/// Get the path to the current log file
pub fn get_current_log_path() -> PathBuf {
    crate::config::get_log_dir().join("protontool.log")
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
