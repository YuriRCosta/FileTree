use fileblade::archive;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[test]
fn all_creation_formats_round_trip_multiple_parents_and_literal_option_names() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir_all(second.join("folder/nested")).unwrap();
    fs::write(first.join("@not-an-archive"), "literal @name").unwrap();
    fs::write(first.join("-C"), "literal option").unwrap();
    fs::write(
        second.join("folder/nested/space #percent%.txt"),
        "nested data",
    )
    .unwrap();
    let sources = [
        first.join("@not-an-archive"),
        first.join("-C"),
        second.join("folder"),
    ]
    .map(|path| path.to_str().unwrap().to_string());
    for format in ["tar", "tar.gz", "tar.zst", "zip"] {
        let target = root.path().join(format!("result.{format}"));
        let result = archive::create(
            &sources,
            target.to_str().unwrap(),
            format,
            &AtomicBool::new(false),
        );
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["undoable"], true, "{result}");
        let extracted = root.path().join(format!("extracted-{format}"));
        fs::create_dir(&extracted).unwrap();
        let output = Command::new("bsdtar")
            .args(["-xf"])
            .arg(&target)
            .arg("-C")
            .arg(&extracted)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fs::read_to_string(extracted.join("@not-an-archive")).unwrap(),
            "literal @name"
        );
        assert_eq!(
            fs::read_to_string(extracted.join("-C")).unwrap(),
            "literal option"
        );
        assert_eq!(
            fs::read_to_string(extracted.join("folder/nested/space #percent%.txt")).unwrap(),
            "nested data"
        );
        assert_no_partial(root.path());
    }
}

#[test]
fn existing_target_self_nested_duplicate_and_remote_sources_are_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join("folder/file"), "source").unwrap();
    let source = root.path().join("folder").to_str().unwrap().to_string();
    let target = root.path().join("existing.tar");
    fs::write(&target, "preserve").unwrap();
    let refused = archive::create(
        std::slice::from_ref(&source),
        target.to_str().unwrap(),
        "tar",
        &AtomicBool::new(false),
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "preserve");
    let target = root.path().join("folder/inside.tar");
    let refused = archive::create(
        std::slice::from_ref(&source),
        target.to_str().unwrap(),
        "tar",
        &AtomicBool::new(false),
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(!target.exists());
    let target = root.path().join("new.tar");
    for sources in [
        vec![source.clone(), source],
        vec!["sftp://host/file".into()],
        vec![],
    ] {
        let refused = archive::create(
            &sources,
            target.to_str().unwrap(),
            "tar",
            &AtomicBool::new(false),
        );
        assert_eq!(refused["ok"], false, "{refused}");
        assert!(!target.exists());
    }
    assert_no_partial(root.path());
}

#[test]
fn cancellation_after_output_started_removes_staging_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("large");
    let mut input = fs::File::create(&source).unwrap();
    let block = vec![b'x'; 1024 * 1024];
    for _ in 0..128 {
        input.write_all(&block).unwrap();
    }
    drop(input);
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let monitor_cancel = Arc::clone(&cancelled);
    let monitor_observed = Arc::clone(&observed);
    let monitor_done = Arc::clone(&done);
    let directory = root.path().to_path_buf();
    let monitor = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !monitor_done.load(Ordering::Relaxed) && Instant::now() < deadline {
            let started = fs::read_dir(&directory).unwrap().flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".fileblade-partial-")
                    && fs::metadata(entry.path().join("archive"))
                        .is_ok_and(|metadata| metadata.len() > 4096)
            });
            if started {
                monitor_observed.store(true, Ordering::Relaxed);
                monitor_cancel.store(true, Ordering::Relaxed);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let target = root.path().join("cancelled.tar");
    let result = archive::create(
        &[source.to_str().unwrap().into()],
        target.to_str().unwrap(),
        "tar",
        &cancelled,
    );
    done.store(true, Ordering::Relaxed);
    monitor.join().unwrap();
    assert!(
        observed.load(Ordering::Relaxed),
        "archive completed without exercising cancellation: {result}"
    );
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["cancelled"], true, "{result}");
    assert!(!target.exists());
    assert!(source.exists());
    assert_no_partial(root.path());
}

fn assert_no_partial(root: &Path) {
    assert!(!fs::read_dir(root).unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with(".fileblade-partial-")
    }));
}

#[test]
#[ignore = "requires an isolated full or read-only test mount"]
fn creation_refuses_a_fault_volume_without_leaving_partial_output() {
    let mount = std::env::var_os("FILEBLADE_ARCHIVE_FAULT_ROOT").expect("fault volume is required");
    let expected =
        std::env::var("FILEBLADE_ARCHIVE_FAULT_ERROR").expect("expected error is required");
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, vec![b'x'; 1024 * 1024]).unwrap();
    let target = Path::new(&mount).join("fault.tar");
    let result = archive::create(
        &[source.to_str().unwrap().into()],
        target.to_str().unwrap(),
        "tar",
        &AtomicBool::new(false),
    );
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"].as_str().unwrap().contains(&expected),
        "{result}"
    );
    assert!(!target.exists());
    assert_eq!(fs::metadata(source).unwrap().len(), 1024 * 1024);
    assert_no_partial(Path::new(&mount));
}
