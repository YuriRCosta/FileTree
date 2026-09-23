use super::*;

#[derive(Clone, Debug, Args)]
pub struct SpaceArgs {
    /// Folder to measure; omitted, the open FileTree root is measured.
    pub path: Option<PathBuf>,
}

pub(super) fn space(options: SpaceArgs) -> AppResult<PublicResult> {
    let path = match options.path {
        Some(path) => {
            let text = path.to_string_lossy();
            if has_scheme(&text) {
                text.into_owned()
            } else {
                path_text(&std::path::absolute(&path)?)
            }
        }
        None => open_root()?,
    };
    let document = backend_json(&["capacity".to_string(), "--path".to_string(), path])?;
    if !document["ok"].as_bool().unwrap_or(false) {
        return Err(AppError::command(error_text(
            &document,
            "drive capacity is unavailable",
        )));
    }
    let line = crate::capacity::describe(&document);
    Ok(PublicResult::lines(vec![line], document))
}

fn has_scheme(text: &str) -> bool {
    text.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
    })
}

fn open_root() -> AppResult<String> {
    let status = object_response("status", &[])
        .map_err(|error| AppError::command(format!("no FileTree window; pass PATH ({error})")))?;
    let flag = |key: &str| status.get(key).and_then(Value::as_bool).unwrap_or(false);
    let root = status
        .get("rootPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let remote = has_scheme(root) && !root[..5].eq_ignore_ascii_case("file:");
    if root.is_empty() || remote || flag("trashMode") || flag("recentMode") || flag("drivesMode") {
        return Err(AppError::command(
            "the current location is not on a local drive; pass PATH",
        ));
    }
    Ok(root.to_string())
}
