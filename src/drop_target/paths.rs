use super::*;

pub(super) fn command_binary(program: &OsStr) -> AppResult<PathBuf> {
    if let Some(text) = program.to_str() {
        if text.starts_with("file://") {
            return Ok(parse_path(text)?);
        }
        if let Some(binary) = which(text) {
            return Ok(binary);
        }
    }
    Ok(PathBuf::from(program))
}

pub(super) fn quoted_paths(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| {
            if path.starts_with("file://") {
                parse_path(path)
                    .map(|path| shell_quote_native(path.as_os_str()))
                    .unwrap_or_else(|_| shell_quote(path))
            } else {
                shell_quote(path)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn configured_launcher(command: &[OsString], cwd: &str) -> AppResult<Vec<String>> {
    let program = crate::common::own_binary()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or_else(|| {
            AppError::invalid("Configured terminal actions need the FileBlade binary")
        })?;
    let folder = parse_path(cwd)?;
    let mut launch = vec![program, EXEC_HEX.to_string()];
    launch.extend(
        std::iter::once(folder.as_os_str())
            .chain(command.iter().map(OsString::as_os_str))
            .map(|value| {
                value
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            }),
    );
    Ok(launch)
}

pub const EXEC_HEX: &str = "exec-hex";

pub fn exec_hex(values: &[String]) -> AppResult<()> {
    let mut decoded = Vec::with_capacity(values.len());
    for value in values {
        decoded.push(decode_hex(value)?);
    }
    let [folder, program, arguments @ ..] = decoded.as_slice() else {
        return Err(AppError::invalid(
            "exec-hex needs a working directory and a program",
        ));
    };
    std::env::set_current_dir(Path::new(OsStr::from_bytes(folder))).map_err(|error| {
        AppError::invalid(format!("exec-hex cannot enter the directory: {error}"))
    })?;
    let error = std::os::unix::process::CommandExt::exec(
        std::process::Command::new(OsStr::from_bytes(program))
            .args(arguments.iter().map(|value| OsStr::from_bytes(value))),
    );
    Err(AppError::invalid(format!(
        "exec-hex cannot run the program: {error}"
    )))
}

fn decode_hex(value: &str) -> AppResult<Vec<u8>> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(AppError::invalid(
            "exec-hex needs even-length hex arguments",
        ));
    }
    bytes
        .chunks(2)
        .map(|pair| {
            let mut byte = 0u8;
            for digit in pair {
                let value = match digit {
                    b'0'..=b'9' => digit - b'0',
                    b'a'..=b'f' => digit - b'a' + 10,
                    _ => return Err(AppError::invalid("exec-hex needs lowercase hex arguments")),
                };
                byte = byte * 16 + value;
            }
            Ok(byte)
        })
        .collect()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(super) fn relative_path(base: &Path, path: &Path) -> Option<PathBuf> {
    let base = fs::canonicalize(base).ok()?;
    let path = fs::canonicalize(path).ok()?;
    let base_parts = base.components().collect::<Vec<_>>();
    let path_parts = path.components().collect::<Vec<_>>();
    let common = base_parts
        .iter()
        .zip(&path_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = PathBuf::new();
    for _ in common..base_parts.len() {
        result.push("..");
    }
    for component in &path_parts[common..] {
        result.push(component.as_os_str());
    }
    Some(result)
}

pub(super) fn command_text(value: &OsStr) -> String {
    value
        .to_str()
        .map(str::to_string)
        .unwrap_or_else(|| display_path(Path::new(value)))
}

pub(super) fn shell_quote_native(value: &OsStr) -> String {
    match value.to_str() {
        Some(text) => shell_quote(text),
        None => format!(
            "$'{}'",
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("\\x{byte:02x}"))
                .collect::<String>()
        ),
    }
}

pub(super) fn native_directory_arg(path: &str) -> AppResult<OsString> {
    let mut argument = OsString::from("--dir=");
    argument.push(parse_path(path)?);
    Ok(argument)
}

pub(super) fn encode_path(value: &Path) -> String {
    value
        .as_os_str()
        .as_bytes()
        .iter()
        .copied()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
