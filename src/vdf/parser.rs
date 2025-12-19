use std::fs;
use std::io;
use std::path::Path;

use super::VDFDict;

/// Errors that can occur during VDF parsing.
#[derive(Debug)]
pub enum VDFError {
    Io(io::Error),
    Parse(String),
}

impl From<io::Error> for VDFError {
    fn from(e: io::Error) -> Self {
        VDFError::Io(e)
    }
}

/// Parse a VDF file from disk into a VDFDict.
pub fn parse_vdf(path: &Path) -> Result<VDFDict, VDFError> {
    let content = fs::read_to_string(path)?;
    parse_vdf_string(&content)
}

/// Parse VDF content from a string into a VDFDict.
///
/// ```
/// use protontool::vdf::parse_vdf_string;
/// let vdf = parse_vdf_string(r#""key" "value""#).unwrap();
/// assert_eq!(vdf.get("key"), Some("value"));
/// ```
pub fn parse_vdf_string(content: &str) -> Result<VDFDict, VDFError> {
    let mut dict = VDFDict::new();
    let mut chars = content.chars().peekable();

    parse_dict(&mut chars, &mut dict)?;

    Ok(dict)
}

/// Skip whitespace characters in the input stream.
fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

/// Parse a quoted string value, handling escape sequences.
fn parse_quoted_string(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Result<String, VDFError> {
    skip_whitespace(chars);

    if chars.next() != Some('"') {
        return Err(VDFError::Parse("Expected opening quote".to_string()));
    }

    let mut result = String::new();
    let mut escaped = false;

    loop {
        match chars.next() {
            None => return Err(VDFError::Parse("Unexpected end of string".to_string())),
            Some('\\') if !escaped => escaped = true,
            Some('"') if !escaped => break,
            Some(c) => {
                escaped = false;
                result.push(c);
            }
        }
    }

    Ok(result)
}

/// Parse a dictionary block (key-value pairs within braces).
fn parse_dict(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    dict: &mut VDFDict,
) -> Result<(), VDFError> {
    loop {
        skip_whitespace(chars);

        match chars.peek() {
            None | Some('}') => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                }
                break;
            }
            Some('"') => {
                let key = parse_quoted_string(chars)?;
                skip_whitespace(chars);

                if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut nested = VDFDict::new();
                    parse_dict(chars, &mut nested)?;
                    dict.insert_dict(key, nested);
                } else {
                    let value = parse_quoted_string(chars)?;
                    dict.insert(key, value);
                }
            }
            Some(c) => {
                return Err(VDFError::Parse(format!("Unexpected character: {}", c)));
            }
        }
    }

    Ok(())
}
