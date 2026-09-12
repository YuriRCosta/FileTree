use fileblade::{
    listing::{self, WindowRequest},
    secure,
};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::symlink;
use std::sync::atomic::AtomicBool;

fn request(path: &std::path::Path) -> WindowRequest {
    WindowRequest {
        path: path.to_string_lossy().into_owned(),
        show_hidden: false,
        start: 0,
        count: 400,
        sort: "name".into(),
        descending: false,
        filter: json!({}),
        include_created: true,
        fresh: false,
        git_enabled: false,
        fresh_git: false,
    }
}

fn names(response: &Value) -> Vec<&str> {
    response["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect()
}

#[test]
fn descriptor_windows_preserve_paging_sort_filter_and_regular_entry_shape() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..1200 {
        fs::write(
            root.path().join(format!("file-{index}.txt")),
            vec![b'x'; index % 9 + 1],
        )
        .unwrap();
    }
    fs::write(root.path().join(".hidden"), "hidden").unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    let directory = secure::open_directory_nofollow(root.path()).unwrap();
    let cancelled = AtomicBool::new(false);
    let mut request = request(root.path());
    for sort in ["name", "size", "modified", "created", "type"] {
        request.sort = sort.into();
        for descending in [false, true] {
            request.descending = descending;
            let ordinary = listing::window(&request, &cancelled);
            let pinned = listing::window_from_directory(&request, &directory, &cancelled);
            assert_eq!(ordinary["ok"], true, "{ordinary}");
            assert_eq!(pinned, ordinary, "sort {sort} desc {descending}");
        }
    }
    fs::write(root.path().join("file-0.txt"), vec![b'x'; 1024]).unwrap();
    listing::invalidate_within(&root.path().join("file-0.txt"));
    request.sort = "size".into();
    request.descending = true;
    request.count = 2;
    let updated = listing::window_from_directory(&request, &directory, &cancelled);
    assert_eq!(updated["entries"][1]["name"], "file-0.txt");
    assert_eq!(updated["entries"][1]["size"], 1024);
    assert_eq!(updated, listing::window(&request, &cancelled));
    request.filter = json!({"size":{"min":4,"max":7},"modified":{"since":"2000-01-01"}});
    request.sort = "size".into();
    request.start = 300;
    request.count = 7;
    assert_eq!(
        listing::window_from_directory(&request, &directory, &cancelled),
        listing::window(&request, &cancelled)
    );
    request.filter = json!({});
    request.show_hidden = true;
    request.start = 1200;
    let tail = listing::window_from_directory(&request, &directory, &cancelled);
    assert_eq!(tail["total"], 1202);
    assert_eq!(tail["entries"].as_array().unwrap().len(), 2);
    assert_eq!(tail["truncated"], false);
    assert_eq!(
        listing::window_from_directory(&request, &directory, &AtomicBool::new(true))["ok"],
        false
    );
}

#[test]
fn descriptor_windows_never_reopen_a_replaced_path_or_follow_child_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("original");
    let decoy = root.path().join("decoy");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&decoy).unwrap();
    fs::write(original.join("original-1"), "old").unwrap();
    fs::write(decoy.join("decoy-secret"), "secret").unwrap();
    symlink(&decoy, original.join("external-link")).unwrap();
    let pinned = secure::open_directory_nofollow(&original).unwrap();
    let mut request = request(&original);
    request.git_enabled = true;
    let cancelled = AtomicBool::new(false);
    let first = listing::window_from_directory(&request, &pinned, &cancelled);
    assert_eq!(names(&first), ["external-link", "original-1"]);
    fs::rename(&original, root.path().join("held")).unwrap();
    symlink(&decoy, &original).unwrap();
    fs::write(root.path().join("held/original-2"), "old-2").unwrap();
    let after = listing::window_from_directory(&request, &pinned, &cancelled);
    assert_eq!(names(&after), ["external-link", "original-1", "original-2"]);
    assert_eq!(after["git_available"], false);
    assert_eq!(after["git_unavailable_reason"], "location-boundary");
    let link = &after["entries"][0];
    assert_eq!(link["is_symlink"], true);
    assert_eq!(link["is_dir"], false);
    assert_eq!(link["kind"], "Symbolic link");
    let replacement = secure::open_directory_nofollow(&decoy).unwrap();
    let changed = listing::window_from_directory(&request, &replacement, &cancelled);
    assert_eq!(names(&changed), ["decoy-secret"]);
    assert_eq!(
        names(&listing::window_from_directory(
            &request, &pinned, &cancelled
        )),
        names(&after)
    );
}
