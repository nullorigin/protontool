# protontool

A comprehensive tool for managing Wine/Proton prefixes with built-in component installation (DLLs, fonts, runtimes), custom verb creation, intelligent error detection, and Steam game integration.

## Features

- **Prefix Management** - Create, configure, and manage Wine/Proton prefixes for Steam games and standalone applications
- **Built-in Verbs** - Install common components like vcrun, dotnet, dxvk, fonts, and more
- **Custom Verbs** - Create and share your own installation verbs via TOML files
- **Smart Logging** - Automatic detection and explanation of Wine errors with a curated database of known issues
- **GUI Support** - Interactive dialogs via zenity or yad for prefix selection, verb installation, and verb creation
- **Steam Integration** - Automatic detection of Steam libraries, games, and Proton versions
- **Proton Compatibility** - Creates prefixes by copying from Proton's default_pfx for proper DLL structure

## Building

```bash
cargo build --release
```

The binaries will be placed in `target/release/`:

- `protontool` - Main CLI tool
- `protontool-launch` - Launch Windows executables  
- `protontool-desktop-install` - Install desktop shortcuts

## Usage

### Install components for a game

```bash
protontool APPID <verb> [verb...]
```

### Search for games

```bash
protontool -s GAME_NAME
```

### List all installed games

```bash
protontool -l
```

### List available verbs

```bash
protontool --list-verbs
```

### Launch GUI

```bash
protontool --gui
```

### Prefix Manager GUI

```bash
protontool -p           # Select from installed games
protontool -p APPID     # Manage specific game's prefix
```

### Create a Custom Prefix

```bash
protontool --create-prefix ~/MyPrefix --proton 'Proton 9.0'
protontool --create-prefix ~/MyPrefix --proton 'Proton 9.0' --arch win32
```

### Delete a Custom Prefix

```bash
protontool --delete-prefix ~/MyPrefix
```

### Run a custom command

```bash
protontool -c "wine myapp.exe" APPID
```

### Launch a Windows executable

```bash
protontool-launch /path/to/app.exe
protontool-launch --appid APPID /path/to/app.exe
```

## Custom Verbs

Create your own installation verbs using TOML files in `~/.protontool/verb/`.

### Example: Simple Verb

```toml
[verb]
name = "myapp"
description = "Install My Application"
category = "apps"

[[actions]]
type = "download"
url = "https://example.com/installer.exe"
filename = "installer.exe"

[[actions]]
type = "run"
executable = "installer.exe"
args = ["/S"]
```

### Example: Advanced Verb with Registry

```toml
[verb]
name = "tweaks"
description = "Apply performance tweaks"
category = "settings"

[[actions]]
type = "reg"
path = "HKCU\\Software\\Wine\\Direct3D"
name = "UseGLSL"
value = "enabled"
```

### Verb Actions

| Action | Description |
|--------|-------------|
| `download` | Download a file from URL |
| `run` | Execute a Windows program |
| `copy` | Copy files to prefix |
| `reg` | Set registry values |
| `override` | Set DLL overrides |
| `winecfg` | Apply winecfg settings |

## Logging

Protontool automatically logs all Wine output and detects known errors:

- Logs stored in `~/.protontool/log/`
- Automatic log rotation (5MB max, keeps 5 backups)
- Known error detection with human-readable explanations
- Covers Wine SEH exceptions, HRESULT codes, NTSTATUS codes, and common patterns

### Example Error Output

```text
┌─ wine ─────────────────────────────────────────
│ Code: WINE-SEH-NODLL
│ Details: DLL not found - missing dependency
└────────────────────────────────────────────────
```

## Compile-time Configuration

Custom paths can be set at compile time using feature flags and environment variables:

```bash
# Custom Steam directory
protontool_DEFAULT_STEAM_DIR=/custom/steam \
  cargo build --features custom_steam_dir

# Custom GUI provider (yad or zenity)
protontool_DEFAULT_GUI_PROVIDER=yad \
  cargo build --features custom_gui_provider

# Custom Steam Runtime path
protontool_STEAM_RUNTIME_PATH=/custom/runtime \
  cargo build --features custom_steam_runtime
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `STEAM_DIR` | Path to custom Steam installation |
| `PROTON_VERSION` | Name of preferred Proton installation |
| `WINE` | Path to custom wine executable |
| `WINESERVER` | Path to custom wineserver executable |
| `STEAM_RUNTIME` | `0` = disable, `1` = enable, or path to custom runtime |
| `protontool_GUI` | GUI provider (`yad` or `zenity`) |

### Variables set by protontool

| Variable | Description |
|----------|-------------|
| `STEAM_APPID` | App ID of the current game |
| `STEAM_APP_PATH` | Path to the game's installation directory |
| `PROTON_PATH` | Path to the Proton installation |

## Directory Structure

Protontool uses `~/.protontool/` for all user data:

```text
~/.protontool/
├── verb/       # Custom verb TOML files
├── pfx/        # Custom (non-Steam) prefixes
├── tmp/        # Temporary downloads
└── log/        # Log files with rotation
```

## Project Structure

```text
src/
├── main.rs              # protontool entry point
├── lib.rs               # Library root
├── bin/
│   ├── launch.rs        # protontool-launch binary
│   ├── desktop_install.rs # Desktop shortcut installer
│   └── wine_extract.rs  # Dev tool for Wine source extraction
├── cli/
│   ├── mod.rs           # CLI logic, GUI handlers, verb creator
│   └── util.rs          # Argument parsing
├── config.rs            # Configuration and path defaults
├── gui.rs               # Zenity/YAD dialog wrappers
├── log.rs               # Logging with error detection
├── wine_data.rs         # Auto-generated Wine debug data
├── steam.rs             # Steam installation detection
├── util.rs              # Utilities (shell_quote, which, etc.)
├── vdf/
│   ├── mod.rs
│   ├── parser.rs        # Valve Data Format parser
│   └── vdict.rs         # VDF dictionary structure
└── wine/
    ├── mod.rs           # Wine module root, WineContext
    ├── prefix.rs        # Prefix initialization (copies from default_pfx)
    ├── verbs.rs         # Built-in verb registry
    ├── custom.rs        # Custom TOML verb loader
    ├── registry.rs      # Windows registry operations
    ├── download.rs      # File download utilities
    └── util.rs          # Wine utilities
```

## Development Tools

### wine-extract

A development tool to extract debug information from Wine/Proton source code and regenerate `wine_data.rs`:

```bash
# Build and run wine-extract
cargo run --bin wine-extract -- \
    --wine-path /path/to/wine \
    --output src/wine_data.rs \
    protontool

# Or regenerate all data
cargo run --bin wine-extract -- \
    --wine-path /path/to/wine \
    --output src/wine_data.rs \
    all
```

This extracts:

- **539+ debug channels** from Wine DLLs
- Curated error patterns for known Wine/Proton issues

Use this when Valve updates their Wine fork to pick up new debug channels.

## Requirements

- Rust 1.70+ (for building)
- Steam with Proton installed
- `zenity` or `yad` (for GUI dialogs)
- `curl` or `wget` (for verb downloads)

## License

MIT
