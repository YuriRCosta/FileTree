use fileblade::thumbnail;
use image::{ImageBuffer, Rgba};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

fn write_png(path: &Path, width: u32, height: u32) {
    let image = ImageBuffer::from_fn(width, height, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
    });
    image.save(path).unwrap();
}

fn png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    (
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    )
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&width.to_be_bytes());
    chunk.extend_from_slice(&height.to_be_bytes());
    chunk.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(&chunk);
    bytes.extend_from_slice(&crc32(&chunk).to_be_bytes());
    let idat = b"IDAT\x78\x9c\x00";
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(idat);
    bytes.extend_from_slice(&crc32(idat).to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(b"IEND");
    bytes.extend_from_slice(&crc32(b"IEND").to_be_bytes());
    bytes
}

#[test]
fn render_scales_a_large_image_down_and_keeps_a_small_one() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let large = root.join("large.png");
    write_png(&large, 3000, 1500);
    let output = root.join("cache/large.png");
    let value = thumbnail::render(large.to_str().unwrap(), &output, 512, 512).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["source_width"], 3000);
    assert_eq!(png_dimensions(&output), (512, 256));
    assert_eq!(value["width"], 512);
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(output.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(!root.join("cache").read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));

    let small = root.join("small.png");
    write_png(&small, 40, 30);
    let output = root.join("cache/small.png");
    let value = thumbnail::render(small.to_str().unwrap(), &output, 512, 512).unwrap();
    assert_eq!(value["width"], 40);
    assert_eq!(png_dimensions(&output), (40, 30));
}

#[test]
fn render_rejects_symlinks_foreign_bytes_and_oversized_headers() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let real = root.join("real.png");
    write_png(&real, 8, 8);
    let link = root.join("link.png");
    symlink(&real, &link).unwrap();
    let output = root.join("cache/out.png");
    let error = thumbnail::render(link.to_str().unwrap(), &output, 64, 64).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("not a regular file") || message.contains("symbolic links"),
        "{message}"
    );

    let fake = root.join("fake.png");
    fs::write(&fake, b"GIF89a not really").unwrap();
    let error = thumbnail::render(fake.to_str().unwrap(), &output, 64, 64).unwrap_err();
    assert!(
        error.to_string().contains("not a PNG, JPEG, or WebP"),
        "{error}"
    );

    let wide = root.join("wide.png");
    fs::write(&wide, png_header_only(200_000, 200_000)).unwrap();
    let error = thumbnail::render(wide.to_str().unwrap(), &output, 64, 64).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("limit") || message.contains("Limits"),
        "{message}"
    );

    let dense = root.join("dense.png");
    fs::write(&dense, png_header_only(16_000, 16_000)).unwrap();
    let error = thumbnail::render(dense.to_str().unwrap(), &output, 64, 64).unwrap_err();
    assert!(error.to_string().contains("above the"), "{error}");
    assert!(!output.exists());
}

#[test]
fn thumbnail_verb_renders_in_a_child_process_and_reuses_the_cache() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let source = root.join("photo.png");
    write_png(&source, 900, 600);
    let cache = root.join("cache");
    let run = |path: &Path, key: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_filetree"))
            .env("XDG_CACHE_HOME", &cache)
            .args([
                "_backend",
                "thumbnail",
                "--path",
                path.to_str().unwrap(),
                "--key",
                key,
                "--width",
                "300",
                "--height",
                "300",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .rev()
            .find(|line| line.starts_with('{'))
            .unwrap();
        serde_json::from_str::<serde_json::Value>(line).unwrap()
    };
    let first = run(&source, "photo\nfp1\n0");
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["width"], 300);
    assert_eq!(first["height"], 200);
    let rendered = Path::new(first["path"].as_str().unwrap()).to_path_buf();
    assert!(rendered.starts_with(cache.join("filetree/thumbnails")));
    assert_eq!(png_dimensions(&rendered), (300, 200));
    assert_eq!(first.get("cached"), None);
    let second = run(&source, "photo\nfp1\n0");
    assert_eq!(second["cached"], true);
    assert_eq!(second["path"], first["path"]);
    let other = run(&source, "photo\nfp2\n0");
    assert_ne!(other["path"], first["path"]);
    let other_source = root.join("other.png");
    write_png(&other_source, 40, 40);
    let same_key = run(&other_source, "photo\nfp1\n0");
    assert_ne!(
        same_key["path"], first["path"],
        "cache keys must also identify the source"
    );
    fs::remove_file(&rendered).unwrap();
    symlink(&source, &rendered).unwrap();
    let linked = run(&source, "photo\nfp1\n0");
    assert_eq!(linked["ok"], false, "{linked}");
    fs::remove_file(&rendered).unwrap();
    fs::write(&rendered, png_header_only(200_000, 200_000)).unwrap();
    fs::set_permissions(&rendered, fs::Permissions::from_mode(0o600)).unwrap();
    let repaired = run(&source, "photo\nfp1\n0");
    assert_eq!(repaired["ok"], true, "{repaired}");
    assert_eq!(repaired.get("cached"), None);
    assert_eq!(png_dimensions(&rendered), (300, 200));

    let missing = run(&root.join("nope.png"), "k");
    assert_eq!(missing["ok"], false);
    assert!(
        missing["error"].as_str().unwrap().contains("missing"),
        "{missing}"
    );

    let cancelled = AtomicBool::new(true);
    let stopped = thumbnail::thumbnail(
        source.to_str().unwrap(),
        "photo\nfp3\n0",
        64,
        64,
        &cancelled,
    );
    assert_eq!(stopped["ok"], false);
}
