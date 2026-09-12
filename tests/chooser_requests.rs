use fileblade::chooser::{Decision, Filter, Mode, Offer, Outcome, Request};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::tempdir;

fn offer(handle: &str, mode: Mode) -> Offer {
    Offer {
        handle: handle.into(),
        caller: format!(":1.{handle}"),
        parent_window: format!("wayland:parent-{handle}"),
        title: handle.into(),
        accept_label: "Choose".into(),
        modal: true,
        current_folder: None,
        current_name: String::new(),
        mode,
        multiple: false,
        filters: vec![Filter("Text".into(), vec![(0, "*.txt".into())])],
        current_filter: None,
    }
}

#[test]
fn simultaneous_offers_cancel_and_complete_independently_once() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("upload #1.txt");
    fs::write(&path, "upload body").unwrap();
    let mut first = Request::new(offer("1", Mode::Open)).unwrap();
    let mut second = Request::new(offer("2", Mode::Open)).unwrap();
    assert!(first.cancel());
    assert!(!first.cancel());
    assert!(
        first
            .choose(std::slice::from_ref(&path), None, false)
            .is_err()
    );
    assert_eq!(second.offer().parent_window, "wayland:parent-2");
    assert_eq!(second.offer().caller, ":1.2");
    let Decision::Finished {
        outcome: Outcome::Accepted { uris, .. },
    } = second
        .choose(std::slice::from_ref(&path), None, false)
        .unwrap()
    else {
        panic!("expected accepted selection")
    };
    assert_eq!(uris.len(), 1);
    assert_eq!(
        url::Url::parse(&uris[0]).unwrap().to_file_path().unwrap(),
        path
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "upload body");
    assert!(!second.cancel());
    assert!(second.choose(&[path], None, false).is_err());
    assert_eq!(first.outcome(), Some(&Outcome::Cancelled));
}

#[test]
fn selection_honors_count_type_and_caller_filters() {
    let directory = tempdir().unwrap();
    let text = directory.path().join("a.txt");
    let image = directory.path().join("a.png");
    fs::write(&text, "text").unwrap();
    fs::write(&image, "not offered").unwrap();
    let mut request = Request::new(offer("1", Mode::Open)).unwrap();
    assert!(
        request
            .choose(&[text.clone(), text.clone()], None, false)
            .is_err()
    );
    assert!(request.choose(&[image], None, false).is_err());
    assert!(
        request
            .choose(&[directory.path().into()], None, false)
            .is_err()
    );
    assert!(
        request
            .choose(&[PathBuf::from("sftp://peer/a.txt")], None, false)
            .is_err()
    );
    assert!(
        request
            .choose(
                std::slice::from_ref(&text),
                Some(&Filter("All".into(), vec![(0, "*".into())])),
                false
            )
            .is_err()
    );
    fs::set_permissions(&text, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(
        request
            .choose(std::slice::from_ref(&text), None, false)
            .is_err()
    );
    fs::set_permissions(&text, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(request.choose(&[text], None, false).is_ok());
    let mut folders = offer("2", Mode::Folder);
    folders.multiple = true;
    let other = directory.path().join("folder");
    fs::create_dir(&other).unwrap();
    let mut folders = Request::new(folders).unwrap();
    let Decision::Finished {
        outcome: Outcome::Accepted { uris, .. },
    } = folders
        .choose(&[directory.path().into(), other], None, false)
        .unwrap()
    else {
        panic!("expected folders")
    };
    assert_eq!(uris.len(), 2);
    assert!(Filter("Images".into(), vec![(1, "image/*".into())]).allows("image", "image/png"));
    assert!(
        !Filter("Images".into(), vec![(1, "image/*".into())]).allows("image", "application/pdf")
    );
}

#[test]
fn overwrite_approval_belongs_to_one_request_and_current_target_identity() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("save.txt");
    fs::write(&path, "original").unwrap();
    let mut first = Request::new(offer("1", Mode::Save)).unwrap();
    let mut second = Request::new(offer("2", Mode::Save)).unwrap();
    assert!(matches!(
        first
            .choose(std::slice::from_ref(&path), None, true)
            .unwrap(),
        Decision::Overwrite { .. }
    ));
    assert!(matches!(
        second
            .choose(std::slice::from_ref(&path), None, true)
            .unwrap(),
        Decision::Overwrite { .. }
    ));
    fs::rename(&path, directory.path().join("old.txt")).unwrap();
    fs::write(&path, "replacement").unwrap();
    assert!(matches!(
        first
            .choose(std::slice::from_ref(&path), None, true)
            .unwrap(),
        Decision::Overwrite { .. }
    ));
    assert!(matches!(
        first
            .choose(std::slice::from_ref(&path), None, true)
            .unwrap(),
        Decision::Finished { .. }
    ));
    assert!(matches!(
        second
            .choose(std::slice::from_ref(&path), None, true)
            .unwrap(),
        Decision::Overwrite { .. }
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
    assert!(second.cancel());
}

#[test]
fn save_new_destination_and_reject_symlink_target() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("new.txt");
    let mut request = Request::new(offer("1", Mode::Save)).unwrap();
    assert!(matches!(
        request
            .choose(std::slice::from_ref(&path), None, false)
            .unwrap(),
        Decision::Finished { .. }
    ));
    assert!(!path.exists());
    let target = directory.path().join("target.txt");
    fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let mut request = Request::new(offer("2", Mode::Save)).unwrap();
    assert!(request.choose(&[path], None, true).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    let mut invalid = offer("3", Mode::Open);
    invalid.filters[0].1[0].0 = 2;
    assert!(Request::new(invalid).is_err());
}
