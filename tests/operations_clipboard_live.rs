use fileblade::{backend, clipboard::Session};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires the D VM clipboard driver and an isolated Wayland session"]
fn owner_offers_real_copy_and_cut_until_the_receiving_manager_pastes() {
    let root = PathBuf::from(
        std::env::var_os("FILEBLADE_CLIPBOARD_PROBE_ROOT").expect("VM fixture root required"),
    );
    assert!(root.is_absolute());
    let _session = Session::open().unwrap();
    for mode in ["copy", "cut"] {
        let source = root.join(mode).join("source/space #percent%.txt");
        let mut args = vec![
            "fileblade",
            "clipboard-write",
            "--path",
            source.to_str().unwrap(),
        ];
        if mode == "cut" {
            args.push("--cut");
        }
        let command = backend::parse(args).unwrap();
        let result = backend::dispatch(command, &AtomicBool::new(false), &mut |_| Ok(())).unwrap();
        assert_eq!(result["ok"], true, "{result}");
        fs::write(
            root.join(format!("{mode}.ready")),
            serde_json::to_vec(&json!({"result":result})).unwrap(),
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        while !root.join(format!("{mode}.pasted")).exists() {
            assert!(
                Instant::now() < until,
                "receiving manager did not confirm {mode}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        let target = root.join(mode).join("destination/space #percent%.txt");
        assert_eq!(fs::read_to_string(target).unwrap(), mode);
        assert_eq!(source.exists(), mode == "copy");
    }
}
