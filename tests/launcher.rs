use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::tempdir;

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn launcher_prefers_the_bundle_and_honors_an_explicit_development_override() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let launcher = root.join("filetree");
    executable(&launcher, include_str!("../filetree"));
    executable(
        &root.join("filetree-bin"),
        "#!/bin/sh\nprintf 'bundle\\n'\n",
    );
    executable(
        &root.join("target/release/filetree"),
        "#!/bin/sh\nprintf 'development\\n'\n",
    );
    executable(
        &root.join("commands/uname"),
        "#!/bin/sh\nprintf 'x86_64\\n'\n",
    );
    let path = format!("{}:/usr/bin:/bin", root.join("commands").display());
    let output = Command::new(&launcher)
        .env_remove("FILETREE_BINARY")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"bundle\n");

    let override_binary = root.join("custom backend");
    executable(&override_binary, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n");
    let output = Command::new(&launcher)
        .env("PATH", &path)
        .env("FILETREE_BINARY", override_binary)
        .args(["one argument", "--flag"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"<one argument>\n<--flag>\n");
}

#[test]
fn another_architecture_skips_the_x86_bundle() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    executable(&root.join("filetree"), include_str!("../filetree"));
    executable(&root.join("filetree-bin"), "#!/bin/sh\nexit 99\n");
    executable(
        &root.join("target/release/filetree"),
        "#!/bin/sh\nprintf 'native\\n'\n",
    );
    executable(
        &root.join("commands/uname"),
        "#!/bin/sh\nprintf 'aarch64\\n'\n",
    );
    let output = Command::new(root.join("filetree"))
        .env_remove("FILETREE_BINARY")
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("commands").display()),
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"native\n");
}
