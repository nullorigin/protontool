//! Wine/Proton source code extractor
//!
//! Extracts debug information from Wine source code and generates Rust tables.
//! Part of protontool - uses shared utilities from the main crate.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use protontool::util::{parse_hex, walk_dir_files_with_ext};

/// CLI arguments
struct Args {
    wine_path: Option<PathBuf>,
    proton_path: Option<PathBuf>,
    output: Option<PathBuf>,
    command: Command,
}

#[derive(Clone, PartialEq)]
enum Command {
    Channels,
    Ntstatus,
    Winerror,
    All,
    Protontool,
    Help,
}

fn print_help() {
    eprintln!(
        r#"wine-extract - Extract debug information from Wine/Proton source code

USAGE:
    wine-extract [OPTIONS] <COMMAND>

OPTIONS:
    -w, --wine-path <PATH>     Path to Wine source directory (auto-detects Proton layout)
    -p, --proton-path <PATH>   Path to Proton repository (uses wine/ subdirectory)
    -o, --output <FILE>        Output file (stdout if not specified)
    -h, --help                 Print help information

COMMANDS:
    channels    Extract debug channel names from Wine DLLs
    ntstatus    Extract NTSTATUS codes from ntstatus.h
    winerror    Extract HRESULT/Win32 error codes from winerror.h
    all         Extract all debug info and generate complete Rust module
    protontool  Generate wine_data.rs module for protontool
"#
    );
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = env::args().collect();

    let mut wine_path = None;
    let mut proton_path = None;
    let mut output = None;
    let mut command = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                return Ok(Args {
                    wine_path: None,
                    proton_path: None,
                    output: None,
                    command: Command::Help,
                });
            }
            "-w" | "--wine-path" => {
                i += 1;
                if i >= args.len() {
                    return Err("--wine-path requires a value".to_string());
                }
                wine_path = Some(PathBuf::from(&args[i]));
            }
            "-p" | "--proton-path" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proton-path requires a value".to_string());
                }
                proton_path = Some(PathBuf::from(&args[i]));
            }
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output requires a value".to_string());
                }
                output = Some(PathBuf::from(&args[i]));
            }
            "channels" => command = Some(Command::Channels),
            "ntstatus" => command = Some(Command::Ntstatus),
            "winerror" => command = Some(Command::Winerror),
            "all" => command = Some(Command::All),
            "protontool" => command = Some(Command::Protontool),
            arg if arg.starts_with('-') => {
                return Err(format!("Unknown option: {}", arg));
            }
            arg => {
                return Err(format!("Unknown command: {}", arg));
            }
        }
        i += 1;
    }

    let command = command.ok_or("No command specified. Use --help for usage.")?;

    Ok(Args {
        wine_path,
        proton_path,
        output,
        command,
    })
}

fn main() -> io::Result<()> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}", e);
            print_help();
            std::process::exit(1);
        }
    };

    if args.command == Command::Help {
        print_help();
        return Ok(());
    }

    let wine_path = resolve_wine_path(&args)?;

    eprintln!("Using Wine source at: {:?}", wine_path);

    let output = match &args.command {
        Command::Channels => extract_channels(&wine_path)?,
        Command::Ntstatus => extract_ntstatus(&wine_path)?,
        Command::Winerror => extract_winerror(&wine_path)?,
        Command::All => generate_all(&wine_path)?,
        Command::Protontool => generate_protontool(&wine_path)?,
        Command::Help => unreachable!(),
    };

    match args.output {
        Some(path) => {
            fs::write(&path, &output)?;
            eprintln!("Written to {:?}", path);
        }
        None => {
            print!("{}", output);
        }
    }

    Ok(())
}

/// Resolve the Wine source path from CLI arguments
fn resolve_wine_path(args: &Args) -> io::Result<PathBuf> {
    if let Some(proton_path) = &args.proton_path {
        if !proton_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Proton path does not exist: {:?}", proton_path),
            ));
        }

        let wine_subdir = proton_path.join("wine");
        if wine_subdir.exists() && wine_subdir.join("dlls").exists() {
            return Ok(wine_subdir);
        }

        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Wine source not found in Proton repository at {:?}/wine",
                proton_path
            ),
        ));
    }

    if let Some(wine_path) = &args.wine_path {
        if !wine_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Wine path does not exist: {:?}", wine_path),
            ));
        }

        if wine_path.join("dlls").exists() {
            return Ok(wine_path.clone());
        }

        let wine_subdir = wine_path.join("wine");
        if wine_subdir.exists() && wine_subdir.join("dlls").exists() {
            eprintln!("Auto-detected Proton repository, using wine/ subdirectory");
            return Ok(wine_subdir);
        }

        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Not a valid Wine source directory: {:?} (missing dlls/ folder)",
                wine_path
            ),
        ));
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "Must specify either --wine-path or --proton-path",
    ))
}

/// Find pattern "WINE_DEFAULT_DEBUG_CHANNEL(name)" and extract name
fn extract_debug_channel(content: &str) -> Vec<String> {
    let mut channels = Vec::new();
    let pattern = "WINE_DEFAULT_DEBUG_CHANNEL";

    for line in content.lines() {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..];
            if let Some(start) = rest.find('(') {
                let after_paren = &rest[start + 1..];
                let end = after_paren
                    .find(|c: char| c == ')' || c.is_whitespace())
                    .unwrap_or(after_paren.len());
                let channel = after_paren[..end].trim();
                if !channel.is_empty() && channel.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    channels.push(channel.to_string());
                }
            }
        }
    }
    channels
}

/// Extract #define STATUS_NAME ((NTSTATUS) 0xXXXXXXXX) patterns
fn extract_ntstatus_defines(content: &str) -> Vec<(String, u32)> {
    let mut results = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("#define") {
            continue;
        }

        if let Some(status_pos) = line.find("STATUS_") {
            let name_start = status_pos;
            let rest = &line[name_start..];
            let name_end = rest
                .find(|c: char| c.is_whitespace() || c == '(')
                .unwrap_or(rest.len());
            let name = &rest[..name_end];

            if line.contains("((NTSTATUS)") || line.contains("(( NTSTATUS )") {
                if let Some(hex_pos) = line.find("0x").or_else(|| line.find("0X")) {
                    let hex_rest = &line[hex_pos..];
                    let hex_end = hex_rest
                        .find(|c: char| !c.is_ascii_hexdigit() && c != 'x' && c != 'X')
                        .unwrap_or(hex_rest.len());
                    if let Some(code) = parse_hex(&hex_rest[..hex_end]) {
                        if code >= 0x80000000 {
                            results.push((name.to_string(), code));
                        }
                    }
                }
            }
        }
    }
    results
}

/// Extract #define E_NAME 0x... or ERROR_NAME patterns
fn extract_winerror_defines(content: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
    let mut hresults = Vec::new();
    let mut win32_errors = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("#define") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let name = parts[1];

        if (name.starts_with("E_") || name.contains("_E_")) && !name.starts_with("ERROR_") {
            let value_part = parts[2..].join(" ");
            if let Some(hex_pos) = value_part.find("0x").or_else(|| value_part.find("0X")) {
                let hex_rest = &value_part[hex_pos..];
                let hex_end = hex_rest
                    .find(|c: char| !c.is_ascii_hexdigit() && c != 'x' && c != 'X')
                    .unwrap_or(hex_rest.len());
                if let Some(code) = parse_hex(&hex_rest[..hex_end]) {
                    if code >= 0x80000000 {
                        hresults.push((name.to_string(), code));
                    }
                }
            }
        }

        if name.starts_with("ERROR_") {
            let value_str = parts[2].trim_end_matches('L');
            if let Ok(code) = value_str.parse::<u32>() {
                if code > 0 && code < 20000 {
                    win32_errors.push((name.to_string(), code));
                }
            }
        }
    }

    (hresults, win32_errors)
}

/// Extract debug channels from WINE_DEFAULT_DEBUG_CHANNEL macros
fn extract_channels(wine_path: &Path) -> io::Result<String> {
    let dlls_path = wine_path.join("dlls");
    if !dlls_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "dlls directory not found in Wine source",
        ));
    }

    let mut channels: BTreeSet<String> = BTreeSet::new();

    eprintln!("Scanning Wine DLLs for debug channels...");

    for path in walk_dir_files_with_ext(&dlls_path, "c") {
        if let Ok(content) = fs::read_to_string(&path) {
            for channel in extract_debug_channel(&content) {
                channels.insert(channel);
            }
        }
    }

    eprintln!("Found {} unique debug channels", channels.len());

    let mut output = String::new();
    output.push_str("/// Wine debug channels extracted from Wine source code\n");
    output.push_str("/// Use with WINEDEBUG=+channel to enable tracing\n");
    output.push_str("pub const WINE_DEBUG_CHANNELS: &[&str] = &[\n");

    let channels_vec: Vec<_> = channels.into_iter().collect();
    for chunk in channels_vec.chunks(8) {
        output.push_str("    ");
        for (i, channel) in chunk.iter().enumerate() {
            output.push_str(&format!("\"{}\"", channel));
            if i < chunk.len() - 1 {
                output.push_str(", ");
            }
        }
        output.push_str(",\n");
    }

    output.push_str("];\n");

    Ok(output)
}

/// Extract NTSTATUS codes from ntstatus.h
fn extract_ntstatus(wine_path: &Path) -> io::Result<String> {
    let ntstatus_path = wine_path.join("include/ntstatus.h");
    if !ntstatus_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "include/ntstatus.h not found in Wine source",
        ));
    }

    let content = fs::read_to_string(&ntstatus_path)?;
    let defines = extract_ntstatus_defines(&content);

    let mut codes: BTreeMap<u32, (String, String)> = BTreeMap::new();
    for (name, code) in defines {
        let description = status_to_description(&name);
        codes.insert(code, (name, description));
    }

    eprintln!("Found {} NTSTATUS error/warning codes", codes.len());

    let mut output = String::new();
    output.push_str("/// NTSTATUS codes extracted from Wine ntstatus.h\n");
    output.push_str("/// Format: (hex_code, name, description)\n");
    output.push_str("pub const NTSTATUS_CODES: &[(u32, &str, &str)] = &[\n");

    for (code, (name, desc)) in &codes {
        output.push_str(&format!(
            "    (0x{:08X}, \"{}\", \"{}\"),\n",
            code, name, desc
        ));
    }

    output.push_str("];\n");

    Ok(output)
}

/// Extract error codes from winerror.h
fn extract_winerror(wine_path: &Path) -> io::Result<String> {
    let winerror_path = wine_path.join("include/winerror.h");
    if !winerror_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "include/winerror.h not found in Wine source",
        ));
    }

    let content = fs::read_to_string(&winerror_path)?;
    let (hresult_list, win32_list) = extract_winerror_defines(&content);

    let mut hresults: BTreeMap<u32, (String, String)> = BTreeMap::new();
    let mut win32_errors: BTreeMap<u32, (String, String)> = BTreeMap::new();

    for (name, code) in hresult_list {
        let description = hresult_to_description(&name);
        hresults.insert(code, (name, description));
    }

    for (name, code) in win32_list {
        let description = error_to_description(&name);
        win32_errors.insert(code, (name, description));
    }

    eprintln!("Found {} HRESULT codes", hresults.len());
    eprintln!("Found {} Win32 error codes", win32_errors.len());

    let mut output = String::new();

    output.push_str("/// HRESULT codes extracted from Wine winerror.h\n");
    output.push_str("pub const HRESULT_CODES: &[(u32, &str, &str)] = &[\n");
    for (code, (name, desc)) in hresults.iter().take(200) {
        output.push_str(&format!(
            "    (0x{:08X}, \"{}\", \"{}\"),\n",
            code, name, desc
        ));
    }
    output.push_str("];\n\n");

    output.push_str("/// Win32 error codes extracted from Wine winerror.h\n");
    output.push_str("pub const WIN32_ERROR_CODES: &[(u32, &str, &str)] = &[\n");
    for (code, (name, desc)) in win32_errors.iter().take(500) {
        output.push_str(&format!("    ({}, \"{}\", \"{}\"),\n", code, name, desc));
    }
    output.push_str("];\n");

    Ok(output)
}

/// Generate complete Rust module with all extracted data
fn generate_all(wine_path: &Path) -> io::Result<String> {
    let mut output = String::new();

    output.push_str("//! Wine debug information extracted from Wine source code\n");
    output.push_str("//! Auto-generated by wine-extract tool\n");
    output.push_str("//! Do not edit manually\n\n");

    output.push_str(&extract_channels(wine_path)?);
    output.push_str("\n");
    output.push_str(&extract_ntstatus(wine_path)?);
    output.push_str("\n");
    output.push_str(&extract_winerror(wine_path)?);

    output.push_str(
        r#"
/// Look up an NTSTATUS code by its hex value
pub fn lookup_ntstatus(code: u32) -> Option<(&'static str, &'static str)> {
    NTSTATUS_CODES.iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, name, desc)| (*name, *desc))
}

/// Look up an HRESULT code by its hex value
pub fn lookup_hresult(code: u32) -> Option<(&'static str, &'static str)> {
    HRESULT_CODES.iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, name, desc)| (*name, *desc))
}

/// Look up a Win32 error code by its numeric value
pub fn lookup_win32_error(code: u32) -> Option<(&'static str, &'static str)> {
    WIN32_ERROR_CODES.iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, name, desc)| (*name, *desc))
}

/// Check if a string is a valid Wine debug channel
pub fn is_valid_channel(channel: &str) -> bool {
    WINE_DEBUG_CHANNELS.contains(&channel)
}
"#,
    );

    output.push_str("\n");
    output.push_str(KNOWN_ERRORS_TEMPLATE);

    Ok(output)
}

/// Convert STATUS_NAME to human-readable description
fn status_to_description(name: &str) -> String {
    let name = name.strip_prefix("STATUS_").unwrap_or(name);
    name.replace('_', " ").to_lowercase()
}

/// Convert E_NAME to human-readable description
fn hresult_to_description(name: &str) -> String {
    let name = name.strip_prefix("E_").unwrap_or(name);
    name.replace('_', " ").to_lowercase()
}

/// Convert ERROR_NAME to human-readable description
fn error_to_description(name: &str) -> String {
    let name = name.strip_prefix("ERROR_").unwrap_or(name);
    name.replace('_', " ").to_lowercase()
}

/// Generate wine_data.rs module for protontool
fn generate_protontool(wine_path: &Path) -> io::Result<String> {
    let dlls_path = wine_path.join("dlls");
    if !dlls_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "dlls directory not found in Wine source",
        ));
    }

    let mut channels: BTreeSet<String> = BTreeSet::new();

    eprintln!("Scanning Wine DLLs for debug channels...");
    for path in walk_dir_files_with_ext(&dlls_path, "c") {
        if let Ok(content) = fs::read_to_string(&path) {
            for channel in extract_debug_channel(&content) {
                channels.insert(channel);
            }
        }
    }
    eprintln!("Found {} unique debug channels", channels.len());

    let mut output = String::new();

    output.push_str(
        r#"//! Wine debug data extracted from Wine source code
//! 
//! This file is auto-generated by the wine-extract tool.
//! Do not edit manually - regenerate with:
//!   cargo run --bin wine-extract -- -w /path/to/wine -o src/wine_data.rs protontool
//!
//! Source: Valve's Wine/Proton fork

"#,
    );

    output.push_str("/// All Wine debug channels extracted from Wine source\n");
    output.push_str("/// Use with WINEDEBUG=+channel to enable tracing\n");
    output.push_str("pub const WINE_DEBUG_CHANNELS: &[&str] = &[\n");

    let channels_vec: Vec<_> = channels.into_iter().collect();
    for chunk in channels_vec.chunks(8) {
        output.push_str("    ");
        for (i, channel) in chunk.iter().enumerate() {
            output.push_str(&format!("\"{}\"", channel));
            if i < chunk.len() - 1 {
                output.push_str(", ");
            }
        }
        output.push_str(",\n");
    }
    output.push_str("];\n\n");

    output.push_str(KNOWN_ERRORS_TEMPLATE);

    output.push_str(
        r#"
/// Check if a string is a valid Wine debug channel
pub fn is_valid_channel(channel: &str) -> bool {
    WINE_DEBUG_CHANNELS.contains(&channel)
}

/// Look up an error by pattern match
pub fn lookup_error(pattern: &str) -> Option<(&'static str, &'static str)> {
    let pattern_lower = pattern.to_lowercase();
    KNOWN_ERRORS.iter()
        .find(|(p, _, _)| pattern_lower.contains(&p.to_lowercase()))
        .map(|(_, code, desc)| (*code, *desc))
}
"#,
    );

    Ok(output)
}

const KNOWN_ERRORS_TEMPLATE: &str = r#"/// Database of known Wine/Windows errors and warnings
/// Format: (pattern to match, error code, description)
pub const KNOWN_ERRORS: &[(&str, &str, &str)] = &[
    // Wine crash/exception codes (from SEH - Structured Exception Handling)
    ("c0000005", "WINE-SEH-AV", "Access violation (STATUS_ACCESS_VIOLATION) - null pointer or bad memory access"),
    ("c0000006", "WINE-SEH-IPF", "In-page I/O error - disk or memory issue"),
    ("c0000008", "WINE-SEH-HANDLE", "Invalid handle - resource already closed or corrupted"),
    ("c000000d", "WINE-SEH-PARAM", "Invalid parameter passed to function"),
    ("c0000017", "WINE-SEH-NOMEM", "No memory available for operation"),
    ("c000001d", "WINE-SEH-ILLEGAL", "Illegal instruction - CPU incompatibility or corruption"),
    ("c0000025", "WINE-SEH-NONCON", "Noncontinuable exception - fatal error"),
    ("c0000026", "WINE-SEH-INVDISP", "Invalid disposition from exception handler"),
    ("c00000fd", "WINE-SEH-STACK", "Stack overflow - infinite recursion or large allocation"),
    ("c0000135", "WINE-SEH-NODLL", "DLL not found - missing dependency"),
    ("c0000138", "WINE-SEH-ORDINAL", "Ordinal not found in DLL - wrong DLL version"),
    ("c0000139", "WINE-SEH-ENTRYPT", "Entry point not found in DLL - API mismatch"),
    ("c0000142", "WINE-SEH-DLLINIT", "DLL initialization failed - check DLL dependencies"),
    ("c0000409", "WINE-SEH-STACKBUF", "Stack buffer overrun detected - security violation"),
    
    // Wine module/loader errors
    ("err:module:import_dll", "WINE-MODULE-001", "Failed to import DLL - check if DLL exists and dependencies are met"),
    ("err:module:load_dll", "WINE-MODULE-002", "Failed to load DLL - file missing, corrupted, or architecture mismatch"),
    ("err:module:attach_dlls", "WINE-MODULE-003", "DLL attach failed during process init"),
    ("err:module:LdrInitializeThunk", "WINE-MODULE-004", "Process initialization failed - critical DLL issue"),
    
    // Wine virtual memory errors
    ("err:virtual:map_file_into_view", "WINE-VIRT-001", "Memory mapping failed - insufficient memory or address space"),
    ("err:virtual:virtual_map_section", "WINE-VIRT-002", "Section mapping failed - memory layout issue"),
    ("err:virtual:allocate_virtual_memory", "WINE-VIRT-003", "Virtual memory allocation failed"),
    
    // Wine ntdll errors
    ("err:ntdll:RtlpWaitForCriticalSection", "WINE-NTDLL-001", "Critical section timeout - possible deadlock"),
    ("err:ntdll:NtTerminateProcess", "WINE-NTDLL-002", "Process termination error"),
    ("fixme:ntdll:NtQuerySystemInformation", "WINE-NTDLL-003", "Unimplemented system info query - usually harmless"),
    ("fixme:ntdll:EtwEventRegister", "WINE-NTDLL-004", "Event tracing not implemented - harmless"),
    
    // Wine display/window errors
    ("err:winediag:nodrv_CreateWindow", "WINE-DISPLAY-001", "No display driver - set DISPLAY env var or check X11/Wayland"),
    ("err:x11drv", "WINE-DISPLAY-002", "X11 driver error - check X server connection"),
    ("err:waylanddrv", "WINE-DISPLAY-003", "Wayland driver error - check Wayland compositor"),
    ("fixme:win:EnumDisplayDevices", "WINE-DISPLAY-004", "Display enumeration incomplete - cosmetic issue"),
    
    // Wine Direct3D/graphics errors
    ("fixme:d3d:", "WINE-D3D-001", "Direct3D feature not implemented - may cause graphical glitches"),
    ("err:d3d:", "WINE-D3D-002", "Direct3D error - graphics issue"),
    ("fixme:d3d11:", "WINE-D3D11-001", "Direct3D 11 feature incomplete"),
    ("fixme:d3d12:", "WINE-D3D12-001", "Direct3D 12 feature incomplete - consider using VKD3D"),
    ("fixme:dxgi:", "WINE-DXGI-001", "DXGI feature incomplete"),
    ("fixme:wined3d:", "WINE-WINED3D-001", "WineD3D implementation incomplete"),
    
    // Wine font/text errors
    ("fixme:dwrite:", "WINE-DWRITE-001", "DirectWrite incomplete - may affect text rendering"),
    ("fixme:font:", "WINE-FONT-001", "Font handling incomplete"),
    ("err:font:", "WINE-FONT-002", "Font error - check font installation"),
    
    // Wine input errors
    ("fixme:dinput:", "WINE-INPUT-001", "DirectInput incomplete - may affect game controllers"),
    ("err:dinput:", "WINE-INPUT-002", "DirectInput error"),
    ("fixme:xinput:", "WINE-XINPUT-001", "XInput incomplete - Xbox controller support"),
    
    // Wine audio errors
    ("err:alsa:", "WINE-AUDIO-001", "ALSA error - check ALSA configuration"),
    ("err:pulse:", "WINE-AUDIO-002", "PulseAudio error - check PulseAudio is running"),
    ("err:mmdevapi:", "WINE-AUDIO-003", "Audio device API error"),
    ("fixme:mmdevapi:", "WINE-AUDIO-004", "Audio API incomplete"),
    ("err:winmm:", "WINE-AUDIO-005", "Windows multimedia error"),
    ("fixme:dsound:", "WINE-AUDIO-006", "DirectSound incomplete"),
    
    // Wine network errors
    ("err:wininet:", "WINE-NET-001", "WinInet error - network/HTTP issue"),
    ("err:winhttp:", "WINE-NET-002", "WinHTTP error - HTTPS/HTTP issue"),
    ("err:winsock:", "WINE-NET-003", "Winsock error - socket/network issue"),
    ("fixme:winsock:", "WINE-NET-004", "Winsock feature incomplete"),
    ("fixme:iphlpapi:", "WINE-NET-005", "IP Helper API incomplete"),
    
    // Wine security/crypto errors
    ("err:crypt:", "WINE-CRYPT-001", "Cryptography error"),
    ("fixme:crypt:", "WINE-CRYPT-002", "Crypto feature incomplete"),
    ("fixme:bcrypt:", "WINE-BCRYPT-001", "BCrypt incomplete - may affect secure operations"),
    ("err:secur32:", "WINE-SEC-001", "Security API error"),
    
    // Wine shell/explorer errors
    ("fixme:shell:", "WINE-SHELL-001", "Shell feature incomplete"),
    ("fixme:explorer:", "WINE-EXPLORER-001", "Explorer feature incomplete"),
    
    // Wine OLE/COM errors
    ("fixme:ole:", "WINE-OLE-001", "OLE/COM feature incomplete"),
    ("err:ole:", "WINE-OLE-002", "OLE/COM error"),
    ("fixme:oleaut:", "WINE-OLEAUT-001", "OLE Automation incomplete"),
    
    // DXVK/VKD3D errors
    ("dxvk: Failed", "DXVK-001", "DXVK translation error - check Vulkan drivers"),
    ("dxvk: Unhandled", "DXVK-002", "DXVK unhandled case"),
    ("vkd3d: Failed", "VKD3D-001", "VKD3D-Proton error - DX12 to Vulkan translation"),
    ("vkd3d-proton: Failed", "VKD3D-002", "VKD3D-Proton error"),
    ("Vulkan: Failed", "VULKAN-001", "Vulkan initialization or operation failed"),
    ("VK_ERROR_", "VULKAN-002", "Vulkan error - check GPU drivers"),
    
    // Windows HRESULT error codes
    ("0x80004001", "HRESULT-E_NOTIMPL", "Not implemented"),
    ("0x80004002", "HRESULT-E_NOINTERFACE", "Interface not supported"),
    ("0x80004003", "HRESULT-E_POINTER", "Invalid pointer"),
    ("0x80004004", "HRESULT-E_ABORT", "Operation aborted"),
    ("0x80004005", "HRESULT-E_FAIL", "Unspecified failure"),
    ("0x80070002", "HRESULT-FILE_NOT_FOUND", "File not found"),
    ("0x80070003", "HRESULT-PATH_NOT_FOUND", "Path not found"),
    ("0x80070005", "HRESULT-E_ACCESSDENIED", "Access denied - check permissions"),
    ("0x8007000e", "HRESULT-E_OUTOFMEMORY", "Out of memory"),
    ("0x80070020", "HRESULT-SHARING_VIOLATION", "File in use by another process"),
    ("0x80070057", "HRESULT-E_INVALIDARG", "Invalid argument"),
    ("0x80070070", "HRESULT-DISK_FULL", "Disk full"),
    ("0x800700aa", "HRESULT-BUSY", "Resource busy"),
    ("0x800706ba", "HRESULT-RPC_UNAVAIL", "RPC server unavailable"),
    ("0x800706be", "HRESULT-RPC_FAILED", "RPC call failed"),
    ("0x80131500", "HRESULT-COR_E_EXCEPTION", ".NET exception"),
    ("0x80131509", "HRESULT-COR_E_INVALIDPROGRAM", "Invalid .NET program"),
    
    // NTSTATUS codes (0xC prefix)
    ("0xc0000005", "NTSTATUS-ACCESS_VIOLATION", "Access violation - memory error"),
    ("0xc000007b", "NTSTATUS-INVALID_IMAGE", "Invalid image format - 32/64-bit mismatch or corruption"),
    ("0xc0000135", "NTSTATUS-DLL_NOT_FOUND", "DLL not found - install required runtime"),
    ("0xc0000139", "NTSTATUS-ENTRYPOINT_NOT_FOUND", "Entry point not found in DLL"),
    ("0xc0000142", "NTSTATUS-DLL_INIT_FAILED", "DLL initialization failed"),
    ("0xc0000409", "NTSTATUS-STACK_BUFFER_OVERRUN", "Stack buffer overrun detected"),
    
    // .NET/CLR errors
    ("CLR error", "DOTNET-CLR-001", "CLR initialization error - install .NET runtime"),
    ("mscorlib", "DOTNET-MSCORLIB", ".NET core library issue"),
    ("System.IO.FileNotFoundException", "DOTNET-FILENOTFOUND", ".NET assembly or file not found"),
    ("System.DllNotFoundException", "DOTNET-DLLNOTFOUND", ".NET P/Invoke DLL not found"),
    ("System.BadImageFormatException", "DOTNET-BADIMAGE", ".NET assembly format error - architecture mismatch"),
    ("System.TypeLoadException", "DOTNET-TYPELOAD", ".NET type loading failed"),
    
    // DirectX errors
    ("D3DERR_INVALIDCALL", "DX-INVALIDCALL", "Invalid Direct3D call"),
    ("DXGI_ERROR_DEVICE_REMOVED", "DX-DEVICE_REMOVED", "GPU device removed - driver crash"),
    ("DXGI_ERROR_DEVICE_RESET", "DX-DEVICE_RESET", "GPU device reset"),
    ("DXGI_ERROR_DRIVER_INTERNAL_ERROR", "DX-DRIVER_ERROR", "GPU driver internal error"),
    ("DXGI_ERROR_NOT_FOUND", "DX-NOT_FOUND", "DXGI resource not found"),
    
    // Generic patterns
    ("Unhandled exception", "CRASH-EXCEPTION", "Unhandled exception - application crashed"),
    ("Segmentation fault", "CRASH-SEGFAULT", "Segmentation fault - memory access error"),
    ("page fault", "CRASH-PAGEFAULT", "Page fault - invalid memory access"),
    ("Assertion failed", "CRASH-ASSERT", "Assertion failure - programming error or corruption"),
    ("Stack overflow", "CRASH-STACKOVERFLOW", "Stack overflow - infinite recursion or deep call stack"),
    ("fatal error", "CRASH-FATAL", "Fatal error occurred"),
    ("cannot find", "ERROR-NOTFOUND", "Required file or resource not found"),
    ("permission denied", "ERROR-PERMISSION", "Permission denied - check file/folder permissions"),
    ("connection refused", "NET-REFUSED", "Network connection refused"),
    ("connection timed out", "NET-TIMEOUT", "Network connection timed out"),
    ("certificate", "NET-CERT", "SSL/TLS certificate issue"),
];
"#;
