use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(manifest: &Path, home: &Path) -> (bool, String) {
    let output = Command::new("sh")
        .arg(root().join("install.sh"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("FILEBLADE_INSTALL_MANIFEST", manifest)
        .output()
        .expect("install.sh runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

fn manifest(directory: &Path, body: &str) -> PathBuf {
    let path = directory.join("latest.json");
    std::fs::write(&path, body).expect("manifest written");
    path
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fileblade-bootstrap-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch created");
    path
}

#[test]
fn the_bootstrap_refuses_a_manifest_without_this_architecture() {
    let directory = scratch("missing-target");
    let path = manifest(
        &directory,
        r#"{"schema":1,"version":"9.9.9","artifacts":{"riscv64-unknown-linux-musl":{"url":"https://example.invalid/a.tar.gz","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}}"#,
    );
    let (ok, text) = run(&path, &directory);
    assert!(!ok, "{text}");
    assert!(text.contains("carries no"), "{text}");
    assert!(!directory.join(".local/bin/fileblade").exists());
}

#[test]
fn the_bootstrap_refuses_a_digest_that_is_not_sha256() {
    let directory = scratch("short-digest");
    let path = manifest(
        &directory,
        r#"{"schema":1,"version":"9.9.9","artifacts":{"x86_64-unknown-linux-musl":{"url":"https://example.invalid/a.tar.gz","sha256":"abc123"},"aarch64-unknown-linux-musl":{"url":"https://example.invalid/a.tar.gz","sha256":"abc123"}}}"#,
    );
    let (ok, text) = run(&path, &directory);
    assert!(!ok, "{text}");
    assert!(text.contains("not a SHA-256 digest"), "{text}");
}

#[test]
fn the_bootstrap_refuses_a_manifest_without_a_version() {
    let directory = scratch("no-version");
    let path = manifest(&directory, r#"{"schema":1,"artifacts":{}}"#);
    let (ok, text) = run(&path, &directory);
    assert!(!ok, "{text}");
    assert!(text.contains("no version"), "{text}");
}

#[test]
fn the_bootstrap_never_reads_a_local_artifact_named_by_a_remote_manifest() {
    let script = std::fs::read_to_string(root().join("install.sh")).expect("bootstrap readable");
    assert!(
        script.contains("a remote manifest may not point at a local artifact"),
        "a remote manifest must not be able to install an attacker-named local path"
    );
    assert!(
        script.contains("the manifest artifact URL is not https"),
        "remote artifacts must be fetched over https"
    );
    assert!(
        script.contains("checksum mismatch"),
        "the downloaded archive must be checked against the manifest digest"
    );
    assert!(
        script.contains("tools/native\" verify"),
        "the payload inventory must be verified before it is installed"
    );
}
