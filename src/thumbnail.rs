use crate::command::CommandSpec;
use crate::common::{own_binary, parse_path, path_text};
use crate::paths::xdg_home;
use crate::secure::{self, read_bounded_nofollow};
use crate::{AppError, AppResult};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde_json::{Value, json};
use std::io::Cursor;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub const INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SOURCE_EDGE: u32 = 16_384;
pub const MAX_SOURCE_PIXELS: u64 = 64_000_000;
pub const MAX_OUTPUT_EDGE: u32 = 1024;
pub const DECODE_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(8);
const RENDER_OUTPUT_BYTES: usize = 64 * 1024;
const CACHE_BYTES: usize = 8 * 1024 * 1024;

pub fn cache_dir() -> PathBuf {
    xdg_home("XDG_CACHE_HOME", "~/.cache").join("fileblade/thumbnails")
}

pub fn cache_path(key: &str) -> PathBuf {
    cache_dir().join(format!("{}.png", digest(key)))
}

pub fn thumbnail(
    raw_path: &str,
    key: &str,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Value {
    let path = match parse_path(raw_path) {
        Ok(path) => path,
        Err(error) => return failure(raw_path, &error.to_string()),
    };
    let text = path_text(&path);
    let (width, height) = bounded_size(width, height);
    let version = match source_version(&path) {
        Ok(version) => version,
        Err(error) => return failure(&text, &error.to_string()),
    };
    let output = cache_path(&format!(
        "oriented-v1\n{text}\n{version}\n{key}\n{width}x{height}"
    ));
    if let Some(ready) = cached(&output, width, height) {
        return ready;
    }
    match render_in_child(&path, &output, width, height, cancelled) {
        Ok(value) if source_version(&path).is_ok_and(|current| current == version) => value,
        Ok(_) => failure(&text, "Source changed while creating the thumbnail"),
        Err(error) => failure(&text, &error.to_string()),
    }
}

pub fn render(raw_path: &str, output: &Path, width: u32, height: u32) -> AppResult<Value> {
    let path = parse_path(raw_path)?;
    let text = path_text(&path);
    let (width, height) = bounded_size(width, height);
    confine_memory()?;
    let bytes = read_bounded_nofollow(&path, INPUT_BYTES)
        .map_err(|error| AppError::Invalid(error.to_string()))?
        .ok_or_else(|| AppError::Invalid(format!("{text} is missing")))?;
    let format = image::guess_format(&bytes)
        .ok()
        .filter(|format| {
            matches!(
                format,
                ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
            )
        })
        .ok_or_else(|| AppError::Invalid(format!("{text} is not a PNG, JPEG, or WebP image")))?;
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(DECODE_MEMORY_BYTES / 2);
    reader.limits(limits);
    let (source_width, source_height) = reader
        .into_dimensions()
        .map_err(|error| AppError::Invalid(format!("{text}: {error}")))?;
    if u64::from(source_width) * u64::from(source_height) > MAX_SOURCE_PIXELS {
        return Err(AppError::Invalid(format!(
            "{text} has {source_width}x{source_height} pixels, above the {MAX_SOURCE_PIXELS} limit"
        )));
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(DECODE_MEMORY_BYTES / 2);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| AppError::Invalid(format!("{text}: {error}")))?;
    let orientation = decoder
        .orientation()
        .map_err(|error| AppError::Invalid(format!("{text}: {error}")))?;
    let mut decoded = DynamicImage::from_decoder(decoder)
        .map_err(|error| AppError::Invalid(format!("{text}: {error}")))?;
    decoded.apply_orientation(orientation);
    let scaled = if decoded.width() > width || decoded.height() > height {
        decoded.thumbnail(width, height)
    } else {
        decoded
    };
    let mut encoded = Cursor::new(Vec::new());
    scaled
        .to_rgba8()
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    secure::write_private_atomic(output, encoded.get_ref())
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    Ok(json!({
        "ok": true,
        "path": path_text(output),
        "width": scaled.width(),
        "height": scaled.height(),
        "source_width": source_width,
        "source_height": source_height
    }))
}

fn render_in_child(
    path: &Path,
    output: &Path,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> AppResult<Value> {
    let program = own_binary().map_err(|error| AppError::command(error.to_string()))?;
    let result = CommandSpec::new(program)
        .args([
            "_backend",
            "thumbnail-render",
            "--path",
            &path_text(path),
            "--target",
            &path_text(output),
            "--width",
            &width.to_string(),
            "--height",
            &height.to_string(),
        ])
        .timeout(RENDER_TIMEOUT)
        .limits(RENDER_OUTPUT_BYTES, 16 * 1024)
        .run_cancellable(cancelled)?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let value: Value = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .unwrap_or(Value::Null);
    if result.status.success() && value["ok"] == true {
        return Ok(value);
    }
    let stderr = String::from_utf8_lossy(&result.stderr);
    let detail = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("fileblade: ").to_string())
        .unwrap_or_else(|| match result.status.code() {
            Some(code) => format!("thumbnail helper exited with status {code}"),
            None => "thumbnail helper was stopped".to_string(),
        });
    Err(AppError::command(detail))
}

fn cached(output: &Path, max_width: u32, max_height: u32) -> Option<Value> {
    secure::ensure_private_directory(output.parent()?).ok()?;
    let bytes = secure::read_private_bounded(output, CACHE_BYTES).ok()??;
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 || width > max_width || height > max_height {
        return None;
    }
    Some(
        json!({"ok": true, "path": path_text(output), "width": width, "height": height, "cached": true}),
    )
}

fn bounded_size(width: u32, height: u32) -> (u32, u32) {
    (
        width.clamp(1, MAX_OUTPUT_EDGE),
        height.clamp(1, MAX_OUTPUT_EDGE),
    )
}

fn source_version(path: &Path) -> AppResult<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        AppError::Invalid(format!(
            "{} is missing or unavailable: {error}",
            path_text(path)
        ))
    })?;
    if !metadata.is_file() || metadata.len() > INPUT_BYTES as u64 {
        return Err(AppError::Invalid(
            "Thumbnail source must be a regular file within the input limit".into(),
        ));
    }
    Ok(format!(
        "{}:{}:{}:{}:{}:{}:{}",
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec()
    ))
}

fn confine_memory() -> AppResult<()> {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let limit = Rlimit {
        current: Some(DECODE_MEMORY_BYTES),
        maximum: Some(DECODE_MEMORY_BYTES),
    };
    match setrlimit(Resource::As, limit) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::PERM) | Err(rustix::io::Errno::INVAL) => Ok(()),
        Err(error) => Err(AppError::command(format!(
            "could not confine thumbnail memory: {error}"
        ))),
    }
}

fn digest(key: &str) -> String {
    let mut first: u64 = 0xcbf2_9ce4_8422_2325;
    let mut second: u64 = 0x84222325_cbf29ce4;
    for byte in key.bytes() {
        first ^= u64::from(byte);
        first = first.wrapping_mul(0x0000_0100_0000_01b3);
        second = second.rotate_left(5) ^ u64::from(byte);
        second = second.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    format!("{first:016x}{second:016x}")
}

fn failure(path: &str, error: &str) -> Value {
    json!({"ok": false, "path": path, "error": error})
}
