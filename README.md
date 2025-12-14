# protontool

A tool for managing Wine/Proton prefixes with built-in component installation (DLLs, fonts, settings) for Steam games and custom prefixes.

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

### Launch GUI

```bash
protontool --gui
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

## Project Structure

```rust
src/
├── main.rs              # protontool entry point
├── lib.rs               # library root
├── bin/
│   ├── launch.rs        # protontool-launch
│   └── desktop_install.rs
├── cli/
│   ├── mod.rs           # CLI logic and mode handlers
│   └── util.rs          # Argument parsing, logging
├── config.rs            # Configuration and defaults
├── gui.rs               # Zenity/YAD dialog wrappers
├── steam.rs             # Steam installation detection
├── util.rs              # Utilities (run_command, which, etc.)
├── winetricks/          # Built-in verb installation system
└── vdf/
    ├── mod.rs
    ├── parser.rs        # Valve Data Format parser
    └── vdict.rs         # VDF dictionary structure
```

## Requirements

- Rust 1.70+ (for building)
- Steam
- Proton
- `zenity` or `yad` (for GUI dialogs)

## License

MIT
