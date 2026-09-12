use fileblade::chooser::Mode;
use fileblade::chooser::portal::options::{Options, offer};
use std::os::unix::ffi::OsStrExt;
use zbus::zvariant::{OwnedValue, Value};

fn owned<T>(value: T) -> OwnedValue
where
    T: Into<Value<'static>>,
{
    OwnedValue::try_from(value.into()).unwrap()
}

#[test]
fn portal_options_keep_defaults_filters_and_nul_paths() {
    let mut options = Options::new();
    options.insert("accept_label".into(), owned("Select".to_owned()));
    options.insert("modal".into(), owned(false));
    options.insert("multiple".into(), owned(true));
    options.insert(
        "current_folder".into(),
        owned(b"/tmp/fileblade-folder\0".to_vec()),
    );
    options.insert(
        "filters".into(),
        owned(vec![(
            "Text".to_owned(),
            vec![(0_u32, "*.txt".to_owned()), (1, "text/plain".to_owned())],
        )]),
    );
    options.insert(
        "current_filter".into(),
        owned(("Text".to_owned(), vec![(0_u32, "*.txt".to_owned())])),
    );
    options.insert(
        "choices".into(),
        owned(Vec::<(String, String, Vec<(String, String)>, String)>::new()),
    );
    let result = offer(
        "/org/freedesktop/portal/desktop/request/_1_2/token",
        ":1.2",
        "wayland:parent",
        "Upload",
        false,
        options,
    )
    .unwrap();
    assert_eq!(result.mode, Mode::Open);
    assert!(result.multiple);
    assert!(!result.modal);
    assert_eq!(result.accept_label, "Select");
    assert_eq!(
        result.current_folder.unwrap().as_os_str().as_bytes(),
        b"/tmp/fileblade-folder"
    );
    assert_eq!(result.filters[0].0, "Text");
    assert_eq!(result.filters[0].1.len(), 2);
    assert_eq!(result.current_filter.unwrap().0, "Text");
    assert!(result.current_name.is_empty());
}

#[test]
fn portal_options_derive_save_name_and_reject_invalid_values() {
    let mut options = Options::new();
    options.insert("current_folder".into(), owned(b"/tmp/ignored\0".to_vec()));
    options.insert("current_name".into(), owned("ignored.txt".to_owned()));
    options.insert(
        "current_file".into(),
        owned(b"/tmp/portal-save.txt\0".to_vec()),
    );
    options.insert("multiple".into(), owned(true));
    let result = offer("handle", ":1.2", "", "Save", true, options).unwrap();
    assert_eq!(result.mode, Mode::Save);
    assert!(!result.multiple);
    assert_eq!(result.current_folder.unwrap(), std::path::Path::new("/tmp"));
    assert_eq!(result.current_name, "portal-save.txt");

    let mut options = Options::new();
    options.insert("accept_label".into(), owned(false));
    let error = offer("handle", ":1.2", "", "Open", false, options).unwrap_err();
    assert!(error.contains("accept_label"));

    let mut options = Options::new();
    options.insert(
        "filters".into(),
        owned(vec![("Bad".to_owned(), vec![(2_u32, "*.bad".to_owned())])]),
    );
    let error = offer("handle", ":1.2", "", "Open", false, options).unwrap_err();
    assert!(error.contains("Invalid chooser filter"));

    let mut options = Options::new();
    options.insert(
        "choices".into(),
        owned(vec![(
            "encoding".to_owned(),
            "Encoding".to_owned(),
            vec![("utf8".to_owned(), "Unicode".to_owned())],
            "utf8".to_owned(),
        )]),
    );
    let error = offer("handle", ":1.2", "", "Open", false, options).unwrap_err();
    assert!(error.contains("choices"));

    let mut options = Options::new();
    options.insert("current_folder".into(), owned(b"/tmp/missing-nul".to_vec()));
    let error = offer("handle", ":1.2", "", "Open", false, options).unwrap_err();
    assert!(error.contains("current_folder"));

    let mut options = Options::new();
    options.insert(
        "current_folder".into(),
        owned(b"/tmp/fileblade-\xff\0".to_vec()),
    );
    let error = offer("handle", ":1.2", "", "Open", false, options).unwrap_err();
    assert!(error.contains("UTF-8"));
}
