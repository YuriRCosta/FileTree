use fileblade::{desktop, filesystem, git, modules, project, search};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn main_checkout_omits_worktree_name_but_linked_checkout_keeps_it() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("repository");
    let linked = temporary.path().join("review");
    fs::create_dir(&root).unwrap();
    for args in [
        vec!["init", "-qb", "main"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
        vec!["worktree", "add", "-qb", "review", linked.to_str().unwrap()],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    for (path, name) in [(&root, ""), (&linked, "review")] {
        let metadata = git::git_metadata_batch(&[path.to_str().unwrap().to_string()]);
        let value = &metadata["results"][0]["git"];
        assert_eq!(value["worktree"], name);
        assert_eq!(value["summary"]["worktree"], name);
        let repository = git::git_worktree_status(path.to_str().unwrap(), false).unwrap();
        let mut entry = serde_json::json!({});
        git::decorate_git_entry(&mut entry, &repository, None, Default::default());
        assert_eq!(entry["git_worktree"], name);
    }
}

#[test]
fn repository_summary_tracks_divergence_and_counts_each_changed_path_once() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
    };
    let summary = || {
        git::git_metadata_batch(&[root.to_str().unwrap().to_string()])["results"][0]["git"]["summary"].clone()
    };
    run(&["init", "-qb", "main"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "user.email", "test@example.invalid"]);
    assert!(summary()["ahead"].is_null());
    for name in ["modified", "deleted", "type-change", "# branch.ab +99 -88"] {
        fs::write(root.join(name), name).unwrap();
    }
    run(&["add", "."]);
    run(&["commit", "-qm", "base"]);
    run(&["branch", "upstream"]);
    run(&["branch", "--set-upstream-to=upstream"]);
    assert_eq!(summary()["ahead"], 0);
    assert_eq!(summary()["behind"], 0);
    run(&["commit", "--allow-empty", "-qm", "local"]);
    run(&["checkout", "-q", "upstream"]);
    run(&["commit", "--allow-empty", "-qm", "remote"]);
    run(&["checkout", "-q", "main"]);
    fs::write(root.join("modified"), "staged").unwrap();
    run(&["add", "modified"]);
    fs::write(root.join("modified"), "unstaged").unwrap();
    fs::remove_file(root.join("deleted")).unwrap();
    fs::remove_file(root.join("type-change")).unwrap();
    symlink("modified", root.join("type-change")).unwrap();
    fs::write(root.join("added"), "added").unwrap();
    run(&["add", "added"]);
    fs::write(root.join("untracked\nname"), "new").unwrap();
    run(&["mv", "# branch.ab +99 -88", "new name"]);
    let value = summary();
    assert_eq!(value["ok"], true);
    assert_eq!(value["ahead"], 1);
    assert_eq!(value["behind"], 1);
    assert_eq!(value["modified"], 1);
    assert_eq!(value["new"], 2);
    assert_eq!(value["added"], 1);
    assert_eq!(value["untracked"], 1);
    assert_eq!(value["copied"], 0);
    assert_eq!(value["type_changed"], 1);
    assert_eq!(value["deleted"], 1);
    assert_eq!(value["renamed"], 1);
    run(&["branch", "--unset-upstream"]);
    assert!(summary()["ahead"].is_null());
    assert_eq!(summary()["modified"], 1);
    run(&["add", "-A"]);
    run(&["commit", "-qm", "snapshot"]);
    run(&["checkout", "--detach", "-q"]);
    assert!(summary()["ahead"].is_null());
    run(&["checkout", "-q", "main"]);
    run(&["branch", "conflict"]);
    fs::write(root.join("modified"), "ours\n").unwrap();
    run(&["commit", "-qam", "ours"]);
    run(&["checkout", "-q", "conflict"]);
    fs::write(root.join("modified"), "theirs\n").unwrap();
    run(&["commit", "-qam", "theirs"]);
    run(&["checkout", "-q", "main"]);
    let merge = Command::new("git")
        .args(["merge", "conflict"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!merge.status.success());
    assert_eq!(summary()["conflicted"], 1);
    assert_eq!(summary()["modified"], 0);
}

#[test]
fn bounded_nofollow_reads_and_lean_directory_rows() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    fs::write(root.join("small.txt"), "hello").unwrap();
    fs::write(root.join("large.txt"), "0123456789").unwrap();
    symlink(root.join("small.txt"), root.join("link.txt")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();

    let small = filesystem::read_text(root.join("small.txt").to_str().unwrap(), 5);
    let large = filesystem::read_text(root.join("large.txt").to_str().unwrap(), 5);
    let link = filesystem::read_text(root.join("link.txt").to_str().unwrap(), 5);
    assert_eq!(small["text"], "hello");
    assert!(large["error"].as_str().unwrap().contains("exceeds"));
    assert_eq!(link["ok"], false);

    let listing = filesystem::children(root.to_str().unwrap(), false);
    let empty = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "empty")
        .unwrap();
    assert_eq!(empty["created"], "");
    assert!(empty["is_empty"].is_null());
    assert!(empty["is_empty_known"].is_null());
    let empty_detail = filesystem::stat_path(root.join("empty").to_str().unwrap());
    assert!(empty_detail["entry"]["is_empty"].is_null());
}

#[test]
fn git_status_disables_repository_fsmonitor() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("repository");
    fs::create_dir(&root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("tracked.txt"), "before").unwrap();
    fs::write(root.join("deleted.txt"), "before").unwrap();
    fs::write(root.join("nested/inner.txt"), "before").unwrap();
    fs::write(root.join(".gitignore"), "ignored/\nnoise.log\n").unwrap();
    assert!(
        Command::new("git")
            .args([
                "add",
                ".gitignore",
                "tracked.txt",
                "deleted.txt",
                "nested/inner.txt"
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    for (key, value) in [
        ("user.name", "FileBlade Test"),
        ("user.email", "fileblade@example.invalid"),
    ] {
        assert!(
            Command::new("git")
                .args(["config", key, value])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        Command::new("git")
            .args(["commit", "-qm", "baseline"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    fs::write(root.join("tracked.txt"), "after").unwrap();
    fs::write(root.join("nested/inner.txt"), "after").unwrap();
    fs::remove_file(root.join("deleted.txt")).unwrap();
    fs::write(root.join("untracked.txt"), "new").unwrap();
    fs::write(root.join("added.txt"), "new").unwrap();
    fs::create_dir(root.join("ignored")).unwrap();
    fs::write(root.join("ignored/inside.txt"), "new").unwrap();
    fs::write(root.join("noise.log"), "new").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "added.txt"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let hook = temporary.path().join("fsmonitor-hook");
    let sentinel = temporary.path().join("fsmonitor-hook.ran");
    fs::write(&hook, "#!/bin/sh\ntouch \"${0}.ran\"\nexit 0\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        Command::new("git")
            .args(["config", "core.fsmonitor", hook.to_str().unwrap()])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let repository = git::git_worktree_status(root.to_str().unwrap(), false).unwrap();
    assert!(repository.ok, "{}", repository.error);
    let statuses = repository
        .entries
        .iter()
        .map(|entry| {
            (
                entry.relative.as_str(),
                entry.status.as_str(),
                entry.deleted,
            )
        })
        .collect::<Vec<_>>();
    assert!(statuses.contains(&("tracked.txt", "M", false)));
    assert!(statuses.contains(&("deleted.txt", "D", true)));
    assert!(statuses.contains(&("untracked.txt", "?", false)));
    assert!(statuses.contains(&("added.txt", "A", false)));
    assert!(!sentinel.exists());

    let listing = filesystem::children(root.to_str().unwrap(), false);
    assert_eq!(listing["git"]["modified_count"], 2);
    assert_eq!(listing["git"]["deleted_count"], 1);
    assert_eq!(listing["git"]["new_count"], 2);
    assert_eq!(
        listing["git"]["status_label"],
        "2 modified · 1 deleted · 1 added · 1 untracked"
    );
    let tracked = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "tracked.txt")
        .unwrap();
    assert_eq!(tracked["git_modified_count"], 1);
    assert_eq!(tracked["git_deleted_count"], 0);
    assert_eq!(tracked["git_new_count"], 0);
    let nested = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "nested")
        .unwrap();
    assert_eq!(nested["git_modified_count"], 1);
    assert_eq!(nested["git_deleted_count"], 0);
    assert_eq!(nested["git_new_count"], 0);
    assert_eq!(nested["git_status_label"], "1 modified");
    let named = |name: &str| {
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(named("ignored")["git_ignored"], true);
    assert_eq!(named("noise.log")["git_ignored"], true);
    assert_eq!(named("tracked.txt")["git_ignored"], false);
    assert_eq!(named("untracked.txt")["git_ignored"], false);
    assert_eq!(listing["git"]["ignored"], false);
    assert_eq!(named("tracked.txt")["git_index_status"], "");
    assert_eq!(named("tracked.txt")["git_worktree_status"], "M");

    assert!(
        Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let cached_listing = filesystem::children(root.to_str().unwrap(), false);
    let cached_tracked = cached_listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "tracked.txt")
        .unwrap();
    assert_eq!(cached_tracked["git_index_status"], "");
    assert_eq!(cached_tracked["git_worktree_status"], "M");

    let without_git = filesystem::children_batch_paged(
        &[root.to_string_lossy().into_owned()],
        false,
        &filesystem::ChildrenPage {
            limit: 400,
            sort: "name".to_string(),
            descending: false,
            filter: serde_json::json!({}),
            include_created: false,
            git_enabled: false,
            fresh_git: false,
        },
        &AtomicBool::new(false),
    );
    let plain = &without_git["results"][0];
    assert!(plain.get("git").is_none());
    assert!(plain["entries"].as_array().unwrap().iter().all(|entry| {
        entry["is_git_repo"] == false && entry["git_status"] == "" && entry["git_ignored"] != true
    }));

    let batch = git::git_metadata_batch(&[
        root.join("ignored/inside.txt")
            .to_string_lossy()
            .into_owned(),
        root.join("noise.log").to_string_lossy().into_owned(),
        root.join("tracked.txt").to_string_lossy().into_owned(),
    ]);
    let results = batch["results"].as_array().unwrap();
    assert_eq!(results[0]["git"]["ignored"], true);
    assert_eq!(results[1]["git"]["ignored"], true);
    assert_eq!(results[2]["git"]["ignored"], false);
    assert_eq!(results[2]["git"]["index_status"], "M");
    assert_eq!(results[2]["git"]["worktree_status"], "");

    let refreshed_listing = filesystem::children(root.to_str().unwrap(), false);
    let refreshed_tracked = refreshed_listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "tracked.txt")
        .unwrap();
    assert_eq!(refreshed_tracked["git_index_status"], "M");
    assert_eq!(refreshed_tracked["git_worktree_status"], "");
}

#[test]
fn aggregate_parsers_reject_uncontrolled_inputs() {
    let temporary = tempdir().unwrap();
    let plugin = temporary.path().join("plugin");
    let module = plugin.join("modules").join("files");
    let user = temporary.path().join("user");
    fs::create_dir_all(&module).unwrap();
    fs::create_dir_all(user.join("linked")).unwrap();
    fs::write(module.join("blade.json"), r#"{"id":"files"}"#).unwrap();
    symlink(
        module.join("blade.json"),
        user.join("linked").join("blade.json"),
    )
    .unwrap();
    let payload = modules::discover_blade_modules(plugin.to_str().unwrap(), user.to_str().unwrap());
    assert_eq!(payload["modules"].as_array().unwrap().len(), 1);
    assert!(desktop::valid_desktop_id("org.example.Editor.desktop"));
    assert!(desktop::valid_desktop_id("Outlook (SQLBI).desktop"));
    assert!(!desktop::valid_desktop_id("../org.example.Editor.desktop"));
    assert!(!desktop::valid_desktop_id("--help.desktop"));
}

#[test]
fn project_roots_ignore_shared_sticky_ancestors() {
    let temporary = tempdir().unwrap();
    let ambient = temporary.path().join("ambient");
    let nested = ambient.join("request").join("deep");
    fs::create_dir_all(ambient.join(".git")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    fs::set_permissions(&ambient, fs::Permissions::from_mode(0o1777)).unwrap();
    let payload = project::project_root(nested.to_str().unwrap());
    assert_eq!(payload["root"], nested.to_str().unwrap());
    assert_eq!(payload["inferred"], true);
}

#[test]
fn search_cancellation_is_bounded() {
    let temporary = tempdir().unwrap();
    let cancelled = AtomicBool::new(true);
    let started = Instant::now();
    let payload = search(
        temporary.path().to_str().unwrap(),
        "missing",
        false,
        2000,
        &[],
        search::SearchOptions::default(),
        &cancelled,
    );
    assert_eq!(payload["ok"], false);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn search_honours_case_and_regex_options() {
    let temporary = tempdir().unwrap();
    fs::write(temporary.path().join("Alpha.md"), "a").unwrap();
    fs::write(temporary.path().join("alpha.txt"), "a").unwrap();
    fs::write(temporary.path().join("beta.md"), "b").unwrap();
    let names = |payload: &serde_json::Value| {
        let mut rows = payload["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    let root = temporary.path().to_str().unwrap();
    let relaxed = search_default(root, "alpha", false, 100, &[]);
    assert_eq!(names(&relaxed), ["Alpha.md", "alpha.txt"]);
    let strict = search(
        root,
        "Alpha",
        false,
        100,
        &[],
        search::SearchOptions {
            case_sensitive: true,
            regex: false,
        },
        &AtomicBool::new(false),
    );
    assert_eq!(names(&strict), ["Alpha.md"]);
    let pattern = search(
        root,
        "^[ab].*\\.md$",
        false,
        100,
        &[],
        search::SearchOptions {
            case_sensitive: false,
            regex: true,
        },
        &AtomicBool::new(false),
    );
    assert_eq!(names(&pattern), ["Alpha.md", "beta.md"]);
    let invalid = search(
        root,
        "(",
        false,
        100,
        &[],
        search::SearchOptions {
            case_sensitive: false,
            regex: true,
        },
        &AtomicBool::new(false),
    );
    assert_eq!(invalid["ok"], false);
    assert!(invalid["error"].as_str().unwrap().contains("invalid regex"));
}

fn search_default(
    root: &str,
    query: &str,
    show_hidden: bool,
    limit: usize,
    repository_roots: &[String],
) -> serde_json::Value {
    search(
        root,
        query,
        show_hidden,
        limit,
        repository_roots,
        search::SearchOptions::default(),
        &AtomicBool::new(false),
    )
}

fn search(
    root: &str,
    query: &str,
    show_hidden: bool,
    limit: usize,
    repository_roots: &[String],
    options: search::SearchOptions,
    cancelled: &AtomicBool,
) -> serde_json::Value {
    search::search_streaming(
        &search::SearchRequest {
            root,
            query,
            show_hidden,
            limit,
            repository_roots,
            options,
            fresh: false,
            list: None,
            tree: false,
            git_enabled: true,
        },
        cancelled,
        &mut |_| Ok(()),
    )
}
