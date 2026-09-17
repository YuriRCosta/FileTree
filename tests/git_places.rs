use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

fn git(home: &Path, directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.com",
        ])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments)
        .current_dir(directory)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} in {}: {}",
        directory.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    linked: PathBuf,
    detached: PathBuf,
}

fn fixture() -> Fixture {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("home");
    let remote = root.join("remote.git");
    let repo = root.join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    git(&home, &root, &["init", "--bare", "remote.git"]);
    git(&home, &repo, &["init"]);
    std::fs::write(repo.join("README"), "one\n").unwrap();
    git(&home, &repo, &["add", "README"]);
    git(&home, &repo, &["commit", "-m", "Initial commit"]);
    git(
        &home,
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&home, &repo, &["push", "-u", "origin", "main"]);
    git(&home, &repo, &["push", "origin", "main:remote-only"]);
    git(&home, &repo, &["switch", "-c", "feature"]);
    std::fs::write(repo.join("feature.txt"), "a\n").unwrap();
    git(&home, &repo, &["add", "feature.txt"]);
    git(&home, &repo, &["commit", "-m", "Feature base"]);
    git(&home, &repo, &["push", "-u", "origin", "feature"]);
    std::fs::write(repo.join("feature.txt"), "b\n").unwrap();
    git(&home, &repo, &["commit", "-am", "Feature pushed"]);
    git(&home, &repo, &["push", "origin", "feature"]);
    git(&home, &repo, &["reset", "--hard", "HEAD~1"]);
    std::fs::write(repo.join("feature.txt"), "c\n").unwrap();
    git(&home, &repo, &["commit", "-am", "Feature diverged"]);
    git(&home, &repo, &["branch", "scratch"]);
    git(&home, &repo, &["switch", "main"]);
    git(&home, &repo, &["fetch", "origin"]);
    let linked = root.join("linked");
    let detached = root.join("detached");
    git(
        &home,
        &repo,
        &["worktree", "add", linked.to_str().unwrap(), "feature"],
    );
    git(
        &home,
        &repo,
        &["worktree", "add", "--detach", detached.to_str().unwrap()],
    );
    std::fs::write(linked.join("untracked.txt"), "new\n").unwrap();
    std::fs::write(linked.join("feature.txt"), "edited\n").unwrap();
    std::fs::write(linked.join("README"), "staged\n").unwrap();
    git(&home, &linked, &["add", "README"]);
    Fixture {
        _temporary: temporary,
        home,
        repo,
        linked,
        detached,
    }
}

fn branch<'a>(document: &'a Value, name: &str) -> &'a Value {
    document["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no branch {name} in {document}"))
}

fn worktree<'a>(document: &'a Value, path: &Path) -> &'a Value {
    document["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == path.to_str().unwrap())
        .unwrap_or_else(|| panic!("no worktree {} in {document}", path.display()))
}

fn assert_places(document: &Value, fixture: &Fixture) {
    assert_eq!(document["ok"], true, "{document}");
    assert_eq!(document["root"], fixture.repo.to_str().unwrap());
    assert_eq!(document["current"], "main");
    assert_eq!(document["detached"], false);
    assert_eq!(document["truncated"], false);
    let names: Vec<&str> = document["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
    assert!(!names.contains(&"origin"), "{names:?}");
    let main = branch(document, "main");
    assert_eq!(main["kind"], "both");
    assert_eq!(main["remote"], "origin");
    assert_eq!(main["upstream"], "origin/main");
    assert_eq!(main["current"], true);
    assert_eq!(main["worktree"], fixture.repo.to_str().unwrap());
    assert_eq!(main["subject"], "Initial commit");
    assert_eq!(main["author"], "Fixture");
    assert!(main["at"].as_i64().unwrap() > 1_700_000_000);
    let feature = branch(document, "feature");
    assert_eq!(feature["kind"], "both");
    assert_eq!(feature["ahead"], 1);
    assert_eq!(feature["behind"], 1);
    assert_eq!(feature["gone"], false);
    assert_eq!(feature["current"], false);
    assert_eq!(feature["worktree"], fixture.linked.to_str().unwrap());
    let scratch = branch(document, "scratch");
    assert_eq!(scratch["kind"], "local");
    assert_eq!(scratch["remote"], "");
    assert_eq!(scratch["upstream"], "");
    assert_eq!(scratch["worktree"], "");
    let remote_only = branch(document, "remote-only");
    assert_eq!(remote_only["kind"], "remote");
    assert_eq!(remote_only["remote"], "origin");
    assert_eq!(remote_only["upstream"], "");
    assert_eq!(document["worktrees"].as_array().unwrap().len(), 3);
    let main_tree = worktree(document, &fixture.repo);
    assert_eq!(main_tree["main"], true);
    assert_eq!(main_tree["current"], true);
    assert_eq!(main_tree["branch"], "main");
    assert_eq!(main_tree["dirty"], false);
    assert_eq!(main_tree["head"].as_str().unwrap().len(), 7);
    assert_eq!(main_tree["at"], main["at"]);
    let linked = worktree(document, &fixture.linked);
    assert_eq!(linked["main"], false);
    assert_eq!(linked["current"], false);
    assert_eq!(linked["branch"], "feature");
    assert_eq!(linked["dirty"], true);
    assert_eq!(linked["staged"], 1);
    assert_eq!(linked["unstaged"], 1);
    assert_eq!(linked["untracked"], 1);
    assert_eq!(linked["conflicted"], 0);
    assert_eq!(linked["modified"], 2);
    assert_eq!(linked["added"], 0);
    assert_eq!(linked["deleted"], 0);
    assert_eq!(linked["renamed"], 0);
    assert_eq!(linked["copied"], 0);
    assert_eq!(linked["type_changed"], 0);
    assert_eq!(linked["locked"], false);
    assert_eq!(linked["prunable"], false);
    let detached = worktree(document, &fixture.detached);
    assert_eq!(detached["branch"], "");
    assert_eq!(detached["dirty"], false);
    assert_eq!(detached["at"], main["at"]);
}

#[test]
fn git_places_lists_merged_branches_and_worktrees() {
    let fixture = fixture();
    let cancelled = AtomicBool::new(false);
    let document = fileblade::git::git_places(fixture.repo.to_str().unwrap(), &cancelled);
    assert_places(&document, &fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .args(["_backend", "git-places", "--path"])
        .arg(&fixture.linked)
        .env("HOME", &fixture.home)
        .env("XDG_STATE_HOME", fixture.home.join("state"))
        .env("XDG_CONFIG_HOME", fixture.home.join("config"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let from_linked: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(from_linked["ok"], true, "{from_linked}");
    assert_eq!(from_linked["root"], fixture.linked.to_str().unwrap());
    assert_eq!(from_linked["current"], "feature");
    assert_eq!(branch(&from_linked, "feature")["current"], true);
    assert_eq!(worktree(&from_linked, &fixture.linked)["current"], true);
    assert_eq!(worktree(&from_linked, &fixture.repo)["main"], true);
    let from_detached = fileblade::git::git_places(fixture.detached.to_str().unwrap(), &cancelled);
    assert_eq!(from_detached["detached"], true);
    assert_eq!(
        from_detached["current"],
        worktree(&from_detached, &fixture.detached)["head"]
    );
}

#[test]
fn git_switch_tracks_a_remote_only_branch() {
    let fixture = fixture();
    let cancelled = AtomicBool::new(false);
    let root = fixture.repo.to_str().unwrap();
    let switched = fileblade::git::git_switch(root, "remote-only", &cancelled);
    assert_eq!(switched["ok"], true, "{switched}");
    assert_eq!(
        git(&fixture.home, &fixture.repo, &["branch", "--show-current"]),
        "remote-only"
    );
    assert_eq!(
        git(
            &fixture.home,
            &fixture.repo,
            &["rev-parse", "--abbrev-ref", "remote-only@{upstream}"]
        ),
        "origin/remote-only"
    );
    let document = fileblade::git::git_places(root, &cancelled);
    assert_eq!(branch(&document, "remote-only")["kind"], "both");
    assert_eq!(branch(&document, "remote-only")["current"], true);
    let back = fileblade::git::git_switch(root, "scratch", &cancelled);
    assert_eq!(back["ok"], true, "{back}");
    let refused = fileblade::git::git_switch(root, "feature", &cancelled);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("already"),
        "{refused}"
    );
}

#[test]
fn git_places_refuses_a_directory_without_a_repository() {
    let temporary = tempfile::tempdir().unwrap();
    let document =
        fileblade::git::git_places(temporary.path().to_str().unwrap(), &AtomicBool::new(false));
    assert_eq!(document["ok"], false);
    assert_eq!(document["error"], "no repository here");
}
