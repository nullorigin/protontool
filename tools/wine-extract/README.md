# wine-extract

Development tool to extract debug information from Wine source code and generate Rust tables for use in protontool.

## Usage

```bash
# Extract debug channels only
wine-extract --wine-path /path/to/wine channels

# Extract NTSTATUS codes
wine-extract --wine-path /path/to/wine ntstatus

# Extract HRESULT/Win32 error codes
wine-extract --wine-path /path/to/wine winerror

# Generate complete Rust module with all data
wine-extract --wine-path /path/to/wine --output wine_data.rs all
```

## Output

The tool generates Rust code with:

- `WINE_DEBUG_CHANNELS` - Array of all valid Wine debug channel names
- `NTSTATUS_CODES` - NTSTATUS error codes with names and descriptions
- `HRESULT_CODES` - HRESULT error codes with names and descriptions
- `WIN32_ERROR_CODES` - Win32 error codes with names and descriptions
- Helper functions: `lookup_ntstatus()`, `lookup_hresult()`, `lookup_win32_error()`, `is_valid_channel()`

## Extracted Data Sources

| Data | Source File | Count |
|------|-------------|-------|
| Debug channels | `dlls/*/*.c` (WINE_DEFAULT_DEBUG_CHANNEL) | ~539 |
| NTSTATUS | `include/ntstatus.h` | ~2557 |
| HRESULT | `include/winerror.h` | ~2488 |
| Win32 errors | `include/winerror.h` | ~2717 |

## Future Extensions

This tool can be extended to extract:

- DLL export functions for task manager display
- Registry key definitions
- Window class names
- COM interface definitions
- And more...

## Building

```bash
cd tools/wine-extract
cargo build --release
```
