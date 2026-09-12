use fileblade::chooser::transport::{ChooserArgs, ChooserCommand, run};
use fileblade::chooser::{Mode, Offer};
use serde_json::{Value, json};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

fn command(command: ChooserCommand) -> Value {
    run(ChooserArgs { command }, &AtomicBool::new(false))
}

#[test]
fn resident_offers_watch_selection_and_disconnected_caller_have_one_lifetime() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("upload.txt");
    fs::write(&file, "browser upload").unwrap();
    let (sender, results) = mpsc::channel();
    let mut cancelled = Vec::new();
    for number in 0..3 {
        let token = Arc::new(AtomicBool::new(false));
        cancelled.push(Arc::clone(&token));
        let sender = sender.clone();
        let offer = Offer {
            handle: format!("request-{number}"),
            caller: format!(":1.{number}"),
            parent_window: format!("wayland:parent-{number}"),
            title: "Upload".into(),
            accept_label: "Choose".into(),
            modal: true,
            current_folder: Some(root.path().into()),
            current_name: String::new(),
            mode: Mode::Open,
            multiple: false,
            filters: Vec::new(),
            current_filter: None,
        };
        std::thread::spawn(move || {
            let result = run(
                ChooserArgs {
                    command: ChooserCommand::Offer {
                        document: serde_json::to_string(&offer).unwrap(),
                    },
                },
                &token,
            );
            sender.send((number, result)).unwrap();
        });
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let snapshot = loop {
        let snapshot = command(ChooserCommand::Watch { revision: 0 });
        if snapshot["offers"].as_array().unwrap().len() == 3 {
            break snapshot;
        }
        if Instant::now() > deadline {
            for token in &cancelled {
                token.store(true, Ordering::Relaxed);
            }
            panic!("offers did not become visible");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(snapshot["offers"][1]["parent_window"], "wayland:parent-1");
    let chosen = command(ChooserCommand::Choose {
        handle: "request-0".into(),
        path: vec![file.clone()],
        filter: None,
        overwrite: false,
    });
    assert_eq!(chosen["decision"]["outcome"]["status"], "accepted");
    assert_eq!(
        command(ChooserCommand::Cancel {
            handle: "request-1".into()
        })["cancelled"],
        true
    );
    cancelled[2].store(true, Ordering::Relaxed);
    let mut outcomes = Vec::new();
    for _ in 0..3 {
        outcomes.push(results.recv_timeout(Duration::from_secs(3)).unwrap());
    }
    outcomes.sort_by_key(|(number, _)| *number);
    assert_eq!(outcomes[0].1["outcome"]["status"], "accepted");
    assert_eq!(outcomes[1].1["outcome"]["status"], "cancelled");
    assert_eq!(outcomes[2].1["outcome"]["status"], "cancelled");
    assert!(
        command(ChooserCommand::Watch { revision: 0 })["offers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        command(ChooserCommand::Choose {
            handle: "request-0".into(),
            path: vec![file],
            filter: None,
            overwrite: false
        })["ok"],
        false
    );
    let filtered = command(ChooserCommand::Filter { document: json!({"filter": ["Documents", [[0, "*.pdf"]]], "entries": [{"path":"/tmp/a.pdf","name":"a.pdf","mime":"application/pdf"},{"path":"/tmp/a.png","name":"a.png","mime":"image/png"}]}).to_string() });
    assert_eq!(filtered["paths"], json!(["/tmp/a.pdf"]));
    let token = AtomicBool::new(true);
    assert_eq!(
        run(
            ChooserArgs {
                command: ChooserCommand::Watch { revision: 0 }
            },
            &token
        )["ok"],
        false
    );
}
