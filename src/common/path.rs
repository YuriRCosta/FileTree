use super::{expanded_path, normalize_path};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fmt::Write;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use url::Url;

pub fn path_text(path: &Path) -> String {
    match path.to_str() {
        Some(text) => text.to_string(),
        None => {
            assert!(path.is_absolute(), "actionable paths must be absolute");
            let mut uri = String::from("file://");
            for byte in path.as_os_str().as_bytes() {
                if byte.is_ascii_alphanumeric() || b"-._~/".contains(byte) {
                    uri.push(*byte as char);
                } else {
                    let _ = write!(uri, "%{byte:02X}");
                }
            }
            uri
        }
    }
}

pub fn parse_path(raw: &str) -> io::Result<PathBuf> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid local file path or URI",
        )
    };
    if raw.contains('\0') {
        return Err(invalid());
    }
    if !raw
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        if raw.split_once(':').is_some_and(|(scheme, _)| {
            scheme
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && scheme
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
        }) {
            return Err(invalid());
        }
        return Ok(expanded_path(raw));
    }
    let bytes = raw.as_bytes();
    if !raw[5..].starts_with("//")
        || bytes
            .iter()
            .any(|byte| byte.is_ascii_control() || matches!(*byte, b'\\' | b' '))
    {
        return Err(invalid());
    }
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && !bytes
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(invalid());
        }
    }
    let uri = Url::parse(raw).map_err(|_| invalid())?;
    if uri.scheme() != "file"
        || uri.host_str().is_some_and(|host| host != "localhost")
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(invalid());
    }
    let path = uri.to_file_path().map_err(|_| invalid())?;
    if !path.is_absolute() || path.as_os_str().as_bytes().contains(&0) {
        return Err(invalid());
    }
    Ok(normalize_path(&path))
}

pub fn path_error(raw: &str, error: &io::Error) -> Value {
    json!({"ok": false, "path": raw, "error": error.to_string()})
}

#[test]
fn local_path_parser_refuses_remote_schemes() {
    for uri in [
        "sftp://user@peer.test/files",
        "SFTP://peer.test/",
        "sftp://",
        "mtp://device/",
        "https://peer.test/",
    ] {
        assert_eq!(
            parse_path(uri).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        parse_path("/tmp/name:part").unwrap(),
        PathBuf::from("/tmp/name:part")
    );
    assert_eq!(
        parse_path("file:///tmp/files").unwrap(),
        PathBuf::from("/tmp/files")
    );
}

pub fn display_path(path: &Path) -> String {
    let mut bytes = path.as_os_str().as_bytes();
    let mut text = String::new();
    while !bytes.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(bytes) {
            Ok(value) => (value, 0),
            Err(error) => (
                std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap(),
                error
                    .error_len()
                    .unwrap_or(bytes.len() - error.valid_up_to()),
            ),
        };
        for character in valid.chars() {
            match character {
                '\\' => text.push_str("\\\\"),
                '\n' => text.push_str("\\n"),
                '\r' => text.push_str("\\r"),
                '\t' => text.push_str("\\t"),
                value if value.is_control() => {
                    let _ = write!(text, "\\u{{{:X}}}", value as u32);
                }
                value => text.push(value),
            }
        }
        bytes = &bytes[valid.len()..];
        for byte in &bytes[..invalid] {
            let _ = write!(text, "\\x{byte:02X}");
        }
        bytes = &bytes[invalid..];
    }
    text
}

pub fn parse_display_name(text: &str) -> io::Result<OsString> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename escape; use \\xNN, \\u{NN}, \\n, \\r, \\t or \\\\",
        )
    };
    let mut bytes = Vec::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        let character = if character != '\\' {
            character
        } else {
            match characters.next().ok_or_else(invalid)? {
                '\\' => '\\',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'x' => {
                    let high = characters
                        .next()
                        .and_then(|c| c.to_digit(16))
                        .ok_or_else(invalid)?;
                    let low = characters
                        .next()
                        .and_then(|c| c.to_digit(16))
                        .ok_or_else(invalid)?;
                    bytes.push((high * 16 + low) as u8);
                    continue;
                }
                'u' => {
                    if characters.next() != Some('{') {
                        return Err(invalid());
                    }
                    let mut digits = String::new();
                    loop {
                        let next = characters.next().ok_or_else(invalid)?;
                        if next == '}' {
                            break;
                        }
                        if !next.is_ascii_hexdigit() || digits.len() >= 6 {
                            return Err(invalid());
                        }
                        digits.push(next);
                    }
                    u32::from_str_radix(&digits, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            }
        };
        bytes.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
    }
    Ok(OsString::from_vec(bytes))
}
