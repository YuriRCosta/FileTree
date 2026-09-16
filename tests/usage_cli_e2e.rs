use chrono::{Duration, FixedOffset, NaiveTime, SecondsFormat, TimeZone, Utc};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;

const ZONE: &str = "<+14>-14";

fn local(days_ago: i64) -> (String, String) {
    let zone = FixedOffset::east_opt(14 * 3600).unwrap();
    let day = Utc::now().with_timezone(&zone).date_naive() - Duration::days(days_ago);
    let moment = zone
        .from_local_datetime(&day.and_time(NaiveTime::from_hms_opt(6, 0, 0).unwrap()))
        .unwrap()
        .with_timezone(&Utc);
    (
        day.format("%Y-%m-%d").to_string(),
        moment.to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

fn record(kind: &str, uuid: &str, at: &str, content: Value) -> String {
    json!({"type": kind, "uuid": uuid, "timestamp": at, "cwd": "/work/project", "isSidechain": false,
           "sessionId": "e2e", "message": {"role": kind, "content": content}})
    .to_string()
        + "\n"
}

fn fileblade(root: &Path, arguments: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .args(arguments)
        .current_dir(root.join("project"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("FILEBLADE_APP_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("CLAUDE_CONFIG_DIR", root.join("home/.claude"))
        .env("CODEX_HOME", root.join("home/.codex"))
        .env("TZ", ZONE)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

fn document(root: &Path, arguments: &[&str]) -> Value {
    let (ok, output) = fileblade(root, arguments);
    assert!(ok, "{arguments:?}: {output}");
    serde_json::from_str(&output).unwrap_or_else(|_| panic!("{arguments:?}: {output}"))
}

#[test]
fn usage_verbs_report_local_days_from_transcripts_and_forget_them() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let skill = root.join("project/.claude/skills/pdf/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "---\nname: pdf\ndescription: fixture skill\n---\n").unwrap();
    let (skill_day, skill_at) = local(3);
    let (typed_day, typed_at) = local(2);
    let (mcp_day, mcp_at) = local(1);
    let transcripts = root.join("home/.claude/projects/-work-project");
    fs::create_dir_all(&transcripts).unwrap();
    fs::write(
        transcripts.join("session.jsonl"),
        [
            record("assistant", "a1", &skill_at, json!([{"type": "tool_use", "id": "toolu_skill", "name": "Skill", "input": {"skill": "pdf"}}])),
            record("user", "u1", &typed_at, json!("<command-name>/pdf</command-name>\n<command-args></command-args>")),
            record("user", "u2", &typed_at, json!("<command-name>/clear</command-name>")),
            record("assistant", "a2", &mcp_at, json!([{"type": "tool_use", "id": "toolu_mcp", "name": "mcp__docs__search", "input": {"query": "x"}}])),
        ]
        .concat(),
    )
    .unwrap();
    let today = local(0).0;

    let skills = document(root, &["usage", "skills", "--output", "json"]);
    assert_eq!(skills["ok"], true);
    assert_eq!(skills["kind"], "skill");
    assert_eq!(skills["coverageStart"], skill_day.as_str());
    assert_eq!(skills["until"], today.as_str());
    assert_eq!(skills["ingestPending"], false);
    assert_eq!(
        skills["days"],
        json!([[skill_day, 1, 1, 0, 0, 0], [typed_day, 1, 0, 1, 0, 0]])
    );
    assert!(
        root.join("state/omarchy/fileblade/agent-usage.sqlite3")
            .is_file()
    );
    assert_eq!(
        fileblade(root, &["usage", "skills"]),
        (true, format!("{skill_day}\t1\n{typed_day}\t1\n"))
    );
    let mcp = document(root, &["-o", "json", "usage", "mcp"]);
    assert_eq!(mcp["kind"], "mcp");
    assert_eq!(mcp["days"], json!([[mcp_day, 1, 1, 0, 0, 0]]));

    let forgotten = document(
        root,
        &["usage", "forget", "--before", &typed_day, "-o", "json"],
    );
    assert_eq!(
        forgotten,
        json!({"ok": true, "schemaVersion": 1, "removed": 1})
    );
    let skills = document(root, &["usage", "skills", "-o", "json"]);
    assert_eq!(skills["coverageStart"], typed_day.as_str());
    assert_eq!(skills["days"], json!([[typed_day, 1, 0, 1, 0, 0]]));

    assert_eq!(
        fileblade(root, &["usage", "forget"]),
        (true, "removed 3\n".to_string())
    );
    let skills = document(root, &["usage", "skills", "-o", "json"]);
    assert_eq!(skills["coverageStart"], Value::Null);
    assert_eq!(skills["days"], json!([]));
    assert_eq!(
        document(root, &["usage", "mcp", "-o", "json"])["days"],
        json!([])
    );
}
