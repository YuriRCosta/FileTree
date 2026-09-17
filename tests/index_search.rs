use fileblade::{index, search};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn relatives(payload: &Value) -> Vec<String> {
    payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["relative"].as_str().unwrap().to_string())
        .collect()
}

fn run(root: &Path, query: &str) -> Value {
    let payload = search_default(root.to_str().unwrap(), query, false, 100, &[]);
    assert_eq!(payload["ok"], true, "{payload}");
    assert_eq!(payload["partial"], false);
    payload
}

fn fixture() -> tempfile::TempDir {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    write(root, "README.md", "a");
    write(root, "src/reader.md", "b");
    write(root, "notes/random-eel.txt", "c");
    write(root, ".hidden/hidden-toggle-fixture.fbtest", "d");
    temporary
}

#[test]
fn metadata_filters_are_applied_before_the_candidate_limit() {
    let temporary = tempdir().unwrap();
    let root = temporary.path();
    for index in 0..1000 {
        write(root, &format!("a-{index:04}.bin"), "x");
    }
    write(root, "z-target.png", "image fixture");
    for query in ["type:image", "mime:image/", "format:png"] {
        let response = search_default(root.to_str().unwrap(), query, false, 200, &[]);
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["partial"], false, "{response}");
        assert_eq!(
            relatives(&response),
            ["z-target.png"],
            "{query}: {response}"
        );
    }
}

#[test]
fn fuzzy_terms_rank_contiguous_matches_first_and_carry_spans() {
    let temporary = fixture();
    let payload = run(temporary.path(), "rdme");
    let rows = relatives(&payload);
    assert_eq!(rows, ["README.md", "notes/random-eel.txt"]);
    assert!(!rows.iter().any(|row| row.starts_with(".hidden")));
    assert_eq!(payload["ranked"], true);
    assert_eq!(payload["backend"], "nucleo");
    assert_eq!(payload["entries"][0]["name_spans"], "0-1,3-6");
    assert!(payload["walked"].as_u64().unwrap() >= 4);
}

#[test]
fn fzf_atoms_select_substring_prefix_suffix_and_negation() {
    let temporary = fixture();
    let root = temporary.path();
    assert_eq!(
        relatives(&run(root, "'ead")),
        ["README.md", "src/reader.md"]
    );
    let mut prefixed = relatives(&run(root, "^src"));
    prefixed.sort();
    assert_eq!(prefixed, ["src", "src/reader.md"]);
    let suffix = relatives(&run(root, "md$"));
    assert_eq!(suffix.len(), 2);
    assert!(suffix.iter().all(|row| row.ends_with(".md")));
    assert_eq!(
        relatives(&run(root, "rdme !readme")),
        ["notes/random-eel.txt"]
    );
    assert_eq!(
        relatives(&run(root, "rdme -readme")),
        ["notes/random-eel.txt"]
    );
    assert_eq!(
        relatives(&run(root, "^notes/random-eel.txt$")),
        ["notes/random-eel.txt"]
    );
    assert!(
        run(root, "\"readme\"")["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(relatives(&run(root, "\"README\"")), ["README.md"]);
}

#[test]
fn filter_only_queries_list_alphabetically_and_hidden_toggle_extends_the_index() {
    let temporary = fixture();
    let root = temporary.path();
    let payload = run(root, "type:file");
    assert_eq!(payload["ranked"], false);
    assert_eq!(
        relatives(&payload),
        ["notes/random-eel.txt", "README.md", "src/reader.md"]
    );
    assert_eq!(relatives(&run(root, "type:folder")), ["notes", "src"]);
    assert_eq!(relatives(&run(root, "format:md in:src")), ["src/reader.md"]);
    let hidden = search_default(
        root.to_str().unwrap(),
        "hidden-toggle-fixture",
        true,
        100,
        &[],
    );
    assert_eq!(
        relatives(&hidden),
        [".hidden/hidden-toggle-fixture.fbtest"],
        "{hidden}"
    );
}

#[test]
fn index_respects_gitignore_and_rebuilds_after_invalidation() {
    let temporary = fixture();
    let root = temporary.path();
    let git = |arguments: &[&str]| {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    write(root, ".gitignore", "ignored.txt\n");
    write(root, "ignored.txt", "x");
    write(root, "kept.txt", "x");
    let cached = index::acquire(root, false, false);
    assert_eq!(relatives(&run(root, "txt$ -random")), ["kept.txt"]);
    write(root, "later.txt", "x");
    assert!(!relatives(&run(root, "later")).contains(&"later.txt".to_string()));
    drop(cached);
    index::invalidate_all();
    assert_eq!(relatives(&run(root, "later")), ["later.txt"]);
    let request = search::SearchRequest {
        root: root.to_str().unwrap(),
        query: "kept",
        show_hidden: false,
        limit: 10,
        repository_roots: &[],
        options: search::SearchOptions::default(),
        fresh: true,
        list: None,
        tree: false,
        git_enabled: true,
    };
    let payload = search::search_streaming(&request, &AtomicBool::new(false), &mut |_| Ok(()));
    assert_eq!(relatives(&payload), ["kept.txt"]);
    let without_git = search::search_streaming(
        &search::SearchRequest {
            git_enabled: false,
            ..request
        },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    );
    assert_eq!(without_git["git_repositories"], 0);
    assert_eq!(without_git["entries"][0]["git_status"], "");
    assert_eq!(without_git["entries"][0]["is_git_repo"], false);
}

#[test]
fn missing_roots_report_an_error_instead_of_an_empty_list() {
    let temporary = tempdir().unwrap();
    let missing = temporary.path().join("gone");
    let payload = search_default(missing.to_str().unwrap(), "anything", false, 10, &[]);
    assert_eq!(payload["ok"], false, "{payload}");
    assert!(payload["entries"].as_array().unwrap().is_empty());
}

#[test]
fn content_search_lists_hits_under_their_files_and_honours_globs() {
    if !fileblade::grep::available() {
        return;
    }
    let temporary = fixture();
    let root = temporary.path();
    write(root, "src/lib.rs", "fn alpha() {}\nfn beta() {}\n");
    write(root, "notes/todo.md", "call alpha tomorrow\n");
    let payload = run(root, "content:alpha");
    assert_eq!(payload["content"], true);
    assert_eq!(payload["hits"], 2);
    assert_eq!(
        relatives(&payload),
        [
            "notes/todo.md",
            "notes/todo.md:1",
            "src/lib.rs",
            "src/lib.rs:1"
        ]
    );
    let hit = &payload["entries"][3];
    assert_eq!(hit["kind"], "Match");
    assert_eq!(hit["line"], 1);
    assert_eq!(hit["name"], "fn alpha() {}");
    assert_eq!(hit["name_spans"], "3-8");
    assert_eq!(
        relatives(&run(root, "content:alpha format:md")),
        ["notes/todo.md", "notes/todo.md:1"]
    );
    assert_eq!(
        relatives(&run(root, "content:alpha in:src")),
        ["src/lib.rs", "src/lib.rs:1"]
    );
    assert_eq!(
        relatives(&run(root, "content:alpha todo")),
        ["notes/todo.md", "notes/todo.md:1"]
    );
}

#[test]
fn quick_nav_discovers_folders_and_ranks_zoxide_visits_with_spans() {
    if Command::new("zoxide").arg("--version").output().is_err() {
        return;
    }
    let temporary = tempfile::Builder::new()
        .prefix("fb-ancestor-")
        .tempdir()
        .unwrap();
    let root = temporary.path();
    let home = root.join("home");
    let data = root.join("zo");
    fs::create_dir_all(&data).unwrap();
    for name in ["git/fileblade", "git/fileblade-git", "home/fb", "docs"] {
        fs::create_dir_all(home.join(name)).unwrap();
    }
    fs::create_dir_all(home.join("never-visited/saturn")).unwrap();
    let add = |name: &str, times: usize| {
        for _ in 0..times {
            assert!(
                Command::new("zoxide")
                    .env("_ZO_DATA_DIR", &data)
                    .args(["add", home.join(name).to_str().unwrap()])
                    .status()
                    .unwrap()
                    .success()
            );
        }
    };
    add("git/fileblade", 3);
    add("git/fileblade-git", 1);
    add("home/fb", 1);
    add("docs", 5);
    unsafe { std::env::set_var("_ZO_DATA_DIR", &data) };
    let payload =
        fileblade::quicknav::quicknav_from_root("fb", "", home.to_str().unwrap(), false, 10);
    assert_eq!(payload["ok"], true, "{payload}");
    let names = payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, ["fb", "fileblade", "fileblade-git"], "{payload}");
    assert_eq!(payload["entries"][0]["name_spans"], "0-2");
    assert_eq!(payload["entries"][1]["name_spans"], "0-1,4-5");
    let all = fileblade::quicknav::quicknav_from_root("", "", home.to_str().unwrap(), false, 10);
    assert_eq!(all["entries"][0]["name"], "docs");
    let unseen =
        fileblade::quicknav::quicknav_from_root("saturn", "", home.to_str().unwrap(), false, 10);
    assert_eq!(unseen["entries"][0]["name"], "saturn", "{unseen}");
    assert_eq!(unseen["entries"][0]["score"], 0.0);
}

#[test]
fn scope_everywhere_ranks_plocate_candidates_as_absolute_rows() {
    let probe = Command::new("plocate")
        .args(["-l", "1", "-i", "--", "/"])
        .output();
    if !probe.is_ok_and(|output| output.status.success() && !output.stdout.is_empty()) {
        return;
    }
    let temporary = tempdir().unwrap();
    let payload = search_default(
        temporary.path().to_str().unwrap(),
        "scope:everywhere / -zzzz",
        false,
        20,
        &[],
    );
    assert_eq!(payload["ok"], true, "{payload}");
    assert_eq!(payload["scope"], "everywhere");
    let rows = payload["entries"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|row| row["path"].as_str().unwrap().starts_with('/'))
    );
    assert!(
        payload["filters"]
            .as_str()
            .unwrap()
            .contains("scope:everywhere")
    );
}

#[test]
fn list_search_filters_a_supplied_path_list() {
    let temporary = fixture();
    let root = temporary.path();
    let file = root.join("list.json");
    let document = serde_json::json!({
        "title": "picked",
        "base": root.to_str().unwrap(),
        "paths": [root.join("src/reader.md"), root.join("README.md"), root.join("missing.md")]
    });
    fileblade::secure::write_private_atomic(&file, document.to_string().as_bytes()).unwrap();
    fn request<'a>(root: &'a str, file: &'a str, query: &'a str) -> search::SearchRequest<'a> {
        search::SearchRequest {
            root,
            query,
            show_hidden: false,
            limit: 10,
            repository_roots: &[],
            options: search::SearchOptions::default(),
            fresh: false,
            list: Some(file),
            tree: false,
            git_enabled: true,
        }
    }
    let (root_text, file_text) = (root.to_str().unwrap(), file.to_str().unwrap());
    let cancelled = AtomicBool::new(false);
    let all = search::search_streaming(&request(root_text, file_text, ""), &cancelled, &mut |_| {
        Ok(())
    });
    assert_eq!(all["list"], true, "{all}");
    assert_eq!(all["title"], "picked");
    assert_eq!(relatives(&all), ["README.md", "src/reader.md"]);
    let filtered = search::search_streaming(
        &request(root_text, file_text, "rdr"),
        &cancelled,
        &mut |_| Ok(()),
    );
    assert_eq!(relatives(&filtered), ["src/reader.md"]);
}

#[test]
fn tree_layout_emits_expanded_ancestors_with_depth() {
    let temporary = fixture();
    let root = temporary.path();
    let request = search::SearchRequest {
        root: root.to_str().unwrap(),
        query: "md$",
        show_hidden: false,
        limit: 10,
        repository_roots: &[],
        options: search::SearchOptions::default(),
        fresh: false,
        list: None,
        tree: true,
        git_enabled: true,
    };
    let payload = search::search_streaming(&request, &AtomicBool::new(false), &mut |_| Ok(()));
    assert_eq!(
        relatives(&payload),
        ["README.md", "src", "src/reader.md"],
        "{payload}"
    );
    let rows = payload["entries"].as_array().unwrap();
    assert_eq!(rows[1]["ancestor"], true);
    assert_eq!(rows[1]["depth"], 0);
    assert_eq!(rows[1]["is_dir"], true);
    assert_eq!(rows[2]["depth"], 1);
    assert_eq!(rows[2]["name_spans"], "7-9");
}

fn search_default(
    root: &str,
    query: &str,
    show_hidden: bool,
    limit: usize,
    repository_roots: &[String],
) -> serde_json::Value {
    search::search_streaming(
        &search::SearchRequest {
            root,
            query,
            show_hidden,
            limit,
            repository_roots,
            options: search::SearchOptions::default(),
            fresh: false,
            list: None,
            tree: false,
            git_enabled: true,
        },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    )
}
