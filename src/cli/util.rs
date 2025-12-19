use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

static LOG_LEVEL: AtomicU32 = AtomicU32::new(0);

pub fn get_log_file_path() -> PathBuf {
    let temp_dir = env::temp_dir();
    let pid = process::id();
    temp_dir.join(format!("protontool{}.log", pid))
}

pub fn delete_log_file() {
    let _ = fs::remove_file(get_log_file_path());
}

pub fn enable_logging(level: u32) {
    LOG_LEVEL.store(level, Ordering::SeqCst);

    let label = match level {
        0 => "WARNING",
        1 => "INFO",
        _ => "DEBUG",
    };

    unsafe { env::set_var("protontool_LOG_LEVEL", label) };
}

pub fn log_debug(msg: &str) {
    if LOG_LEVEL.load(Ordering::SeqCst) >= 2 {
        eprintln!("protontool (DEBUG): {}", msg);
    }
}

pub fn log_info(msg: &str) {
    if LOG_LEVEL.load(Ordering::SeqCst) >= 1 {
        eprintln!("protontool (INFO): {}", msg);
    }
}

pub fn log_warning(msg: &str) {
    eprintln!("protontool (WARNING): {}", msg);
}

pub fn exit_with_error(error: &str, desktop: bool) -> ! {
    if !desktop {
        eprintln!("{}", error);
        process::exit(1);
    }

    let log_messages = fs::read_to_string(get_log_file_path())
        .unwrap_or_else(|_| "!! LOG FILE NOT FOUND !!".to_string());

    let is_steam_deck = crate::steam::is_steam_deck();
    let is_steamos = crate::steam::is_steamos();

    let message = format!(
        "protontool was closed due to the following error:\n\n\
         {}\n\n\
         =============\n\n\
         Please include this entire error message when making a bug report.\n\
         Environment:\n\n\
         protontool version: {}\n\
         Is Steam Deck: {}\n\
         Is SteamOS 3+: {}\n\n\
         Log messages:\n\n\
         {}",
        error,
        crate::VERSION,
        is_steam_deck,
        is_steamos,
        log_messages
    );

    crate::gui::show_text_dialog("protontool", &message);
    process::exit(1);
}

#[derive(Debug, Clone)]
pub struct ArgDef {
    pub name: String,
    pub flags: Vec<String>,
    pub help: String,
    pub is_option: bool,
    pub is_multi: bool,
}

#[derive(Debug)]
pub struct ParsedArgs {
    flags: HashMap<String, u32>,
    options: HashMap<String, String>,
    multi_options: HashMap<String, Vec<String>>,
    positional: Vec<String>,
}

impl Default for ParsedArgs {
    fn default() -> Self {
        Self {
            flags: HashMap::new(),
            options: HashMap::new(),
            multi_options: HashMap::new(),
            positional: Vec::new(),
        }
    }
}

impl ParsedArgs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_flag(&self, name: &str) -> bool {
        self.flags.get(name).is_some_and(|&v| v > 0)
    }

    pub fn get_count(&self, name: &str) -> u32 {
        self.flags.get(name).copied().unwrap_or(0)
    }

    pub fn get_option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(|s| s.as_str())
    }

    pub fn get_multi_option(&self, name: &str) -> &[String] {
        self.multi_options.get(name).map_or(&[], |v| v.as_slice())
    }

    pub fn positional(&self) -> &[String] {
        &self.positional
    }
}

pub struct ArgParser {
    prog: String,
    description: String,
    args: Vec<ArgDef>,
}

impl ArgParser {
    pub fn new(prog: &str, description: &str) -> Self {
        Self {
            prog: prog.to_string(),
            description: description.to_string(),
            args: Vec::new(),
        }
    }

    pub fn add_flag(&mut self, name: &str, flags: &[&str], help: &str) {
        self.args.push(ArgDef {
            name: name.to_string(),
            flags: flags.iter().map(|s| s.to_string()).collect(),
            help: help.to_string(),
            is_option: false,
            is_multi: false,
        });
    }

    pub fn add_option(&mut self, name: &str, flags: &[&str], help: &str) {
        self.args.push(ArgDef {
            name: name.to_string(),
            flags: flags.iter().map(|s| s.to_string()).collect(),
            help: help.to_string(),
            is_option: true,
            is_multi: false,
        });
    }

    pub fn add_multi_option(&mut self, name: &str, flags: &[&str], help: &str) {
        self.args.push(ArgDef {
            name: name.to_string(),
            flags: flags.iter().map(|s| s.to_string()).collect(),
            help: help.to_string(),
            is_option: true,
            is_multi: true,
        });
    }

    pub fn parse(&self, args: &[String]) -> Result<ParsedArgs, String> {
        let mut parsed = ParsedArgs::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];

            if arg.starts_with('-') {
                let mut found = false;

                for def in &self.args {
                    if def.flags.iter().any(|f| f == arg) {
                        found = true;
                        if def.is_option {
                            i += 1;
                            if i >= args.len() {
                                return Err(format!("Option {} requires a value", arg));
                            }
                            if def.is_multi {
                                parsed.multi_options.entry(def.name.clone()).or_default().push(args[i].clone());
                            } else {
                                parsed.options.insert(def.name.clone(), args[i].clone());
                            }
                        } else {
                            let count = parsed.flags.get(&def.name).copied().unwrap_or(0);
                            parsed.flags.insert(def.name.clone(), count + 1);
                        }
                        break;
                    }
                }

                if !found {
                    return Err(format!("Unknown option: {}", arg));
                }
            } else {
                parsed.positional.push(arg.clone());
            }

            i += 1;
        }

        Ok(parsed)
    }

    pub fn help(&self) -> String {
        let mut help = format!("{}\n\n{}\n\nOptions:\n", self.prog, self.description);

        for arg in &self.args {
            let flags_str = arg.flags.join(", ");
            help.push_str(&format!("  {:24} {}\n", flags_str, arg.help));
        }

        help
    }
}
