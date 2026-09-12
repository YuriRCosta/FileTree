use super::*;

pub(super) fn actions_for(target: &Value, facts: &Value) -> Vec<Value> {
    let mut rows = target_actions(target, facts);
    rows.extend(generic_actions(facts));
    if resolved_multiplexer(target).is_some()
        && let Some(review) = rows.iter_mut().find(|row| row["id"] == "review")
    {
        *review = action(
            "review",
            "Review with hunk",
            "r",
            "󰹃",
            "Review changes in a new pane, tab, space, or terminal window",
            &REVIEW_PLACEMENTS,
            "open",
        );
    }
    for row in &mut rows {
        if let Some(placements) = row.get_mut("placements").and_then(Value::as_array_mut) {
            assign_keys(placements);
        }
    }
    assign_keys(&mut rows);
    rows
}

pub(super) fn target_actions(target: &Value, facts: &Value) -> Vec<Value> {
    match target
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("desktop")
    {
        "terminal" => terminal_actions(target, facts),
        "editor" => editor_actions(target, facts),
        "app" | "browser" => app_actions(target),
        _ => Vec::new(),
    }
}

pub(super) fn terminal_actions(target: &Value, facts: &Value) -> Vec<Value> {
    match resolved_multiplexer(target) {
        Some("herdr") => multiplexer_actions(facts, &HERDR_PLACEMENTS, "herdr"),
        Some("tmux") => multiplexer_actions(facts, &TMUX_PLACEMENTS, "tmux"),
        _ => Vec::new(),
    }
}

pub(super) fn ambiguous_reason(target: &Value) -> Option<String> {
    let terminal = target
        .get("terminal")
        .filter(|value| value.get("ambiguous").and_then(Value::as_bool) == Some(true))
        .or_else(|| {
            target
                .get("editor")
                .filter(|value| value.get("ambiguous").and_then(Value::as_bool) == Some(true))
        })?;
    Some(
        terminal
            .get("reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
            .unwrap_or("cannot identify this window's pane")
            .to_string(),
    )
}

pub(super) fn resolved_multiplexer(target: &Value) -> Option<&str> {
    let terminal = target.get("terminal").unwrap_or(&Value::Null);
    if terminal.get("ambiguous").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    match terminal
        .get("multiplexer")
        .and_then(Value::as_str)
        .unwrap_or("none")
    {
        "herdr"
            if terminal
                .get("pane_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) =>
        {
            Some("herdr")
        }
        "tmux"
            if terminal
                .get("session")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) =>
        {
            Some("tmux")
        }
        _ => None,
    }
}

pub(super) fn multiplexer_actions(
    facts: &Value,
    placements: &[Placement],
    name: &str,
) -> Vec<Value> {
    let with_files = has_files(facts);
    let description = if with_files {
        format!(
            "Open in nvim inside a new {name} {}",
            placement_list(placements)
        )
    } else {
        format!(
            "Open a shell in the folder inside a new {name} {}",
            placement_list(placements)
        )
    };
    let mut row = action(
        "mux-open",
        name,
        &name[..1],
        if with_files { "󰕷" } else { "󱆃" },
        &description,
        placements,
        "target",
    );
    row["icon"] = json!(name);
    row["icon_mask"] = json!(multiplexer_icon_mask(name));
    vec![row]
}

fn multiplexer_icon_mask(name: &str) -> &'static str {
    match name {
        "herdr" => "dark",
        "tmux" => "ink",
        _ => "",
    }
}

pub(super) fn placement_list(placements: &[Placement]) -> String {
    let names = placements
        .iter()
        .map(|placement| {
            placement
                .1
                .strip_prefix("New ")
                .unwrap_or(placement.1)
                .to_lowercase()
        })
        .collect::<Vec<_>>();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

pub(super) fn has_files(facts: &Value) -> bool {
    facts
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty())
}

pub(super) fn editor_actions(target: &Value, facts: &Value) -> Vec<Value> {
    let editor = target.get("editor").unwrap_or(&Value::Null);
    if editor.get("kind").and_then(Value::as_str) == Some("nvim")
        && editor
            .get("server")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
        && has_files(facts)
    {
        let mut row = action(
            "nvim-open",
            "This nvim",
            "n",
            "󰕷",
            "Open in this nvim window",
            &NVIM_PLACEMENTS,
            "target",
        );
        row["icon"] = json!("nvim");
        row["desktop_id"] = json!("nvim.desktop");
        vec![row]
    } else {
        Vec::new()
    }
}

pub(super) fn app_actions(target: &Value) -> Vec<Value> {
    let app = target.get("app").unwrap_or(&Value::Null);
    let desktop_id = app
        .get("desktop_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if desktop_id.is_empty()
        || !app
            .get("accepts_files")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Vec::new();
    }
    let name = app
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("app");
    let mut row = action(
        "app-open",
        name,
        "",
        "󰏗",
        &format!("Open in {name}"),
        &[],
        "target",
    );
    row["desktop_id"] = json!(desktop_id);
    row["icon"] = json!(app.get("icon").and_then(Value::as_str).unwrap_or_default());
    row["icon_source"] = json!(
        app.get("icon_source")
            .and_then(Value::as_str)
            .unwrap_or_default()
    );
    row["icon_mask"] = json!(
        app.get("icon_mask")
            .and_then(Value::as_str)
            .unwrap_or_default()
    );
    vec![row]
}

pub(super) fn generic_actions(facts: &Value) -> Vec<Value> {
    let directories = facts
        .get("directories")
        .and_then(Value::as_array)
        .is_some_and(|directories| !directories.is_empty());
    let (open_label, open_description) = match (has_files(facts), directories) {
        (false, true) => ("Open in FileBlade", "Browse the folder in FileBlade"),
        (true, true) => (
            "Open in new window",
            "Open files with their default applications and folders in FileBlade",
        ),
        _ => (
            "Open in new window",
            "Open in a new window of the default application",
        ),
    };
    let mut rows = vec![action(
        "open",
        open_label,
        "o",
        "󰏌",
        open_description,
        &[],
        "open",
    )];
    let mut open_with = action(
        "open-with",
        "Open with",
        "w",
        "󰝰",
        "Choose an application for these files",
        &[],
        "open",
    );
    open_with["placements"] = Value::Array(open_with_applications(facts));
    rows.push(open_with);
    rows.push(action(
        "terminal",
        "New terminal",
        "t",
        "󰆍",
        "Open a shell in a new terminal window",
        &[],
        "open",
    ));
    if review_possible(facts) {
        rows.push(action(
            "review",
            "Review with hunk",
            "r",
            "󰹃",
            "Review the changes with hunk in a new terminal",
            &[],
            "open",
        ));
    }
    rows
}

pub(super) fn action(
    id: &str,
    label: &str,
    key: &str,
    glyph: &str,
    description: &str,
    placements: &[Placement],
    group: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "key": key,
        "glyph": glyph,
        "description": description,
        "placements": placements.iter().map(|(id, label, key, glyph, description)| json!({
            "id": id,
            "label": label,
            "key": key,
            "glyph": glyph,
            "icon": match *id {
                "right" => "pane-vertical",
                "down" => "pane-horizontal",
                _ => "",
            },
            "icon_mask": "ink",
            "description": description,
        })).collect::<Vec<_>>(),
        "group": group,
    })
}

pub(super) fn assign_keys(rows: &mut [Value]) {
    let mut used = HashSet::new();
    for row in rows.iter_mut() {
        let wanted = row
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let mut letters = wanted.chars().filter(|value| value.is_alphanumeric());
        match (letters.next(), letters.next()) {
            (Some(letter), None) if !used.contains(&letter) => {
                used.insert(letter);
                row["key"] = Value::String(letter.to_string());
            }
            _ => row["key"] = Value::String(String::new()),
        }
    }
    for row in rows.iter_mut() {
        if row.get("key").and_then(Value::as_str) != Some("") {
            continue;
        }
        let label = row
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let chosen = label
            .chars()
            .filter(|value| value.is_alphanumeric())
            .find(|value| !used.contains(value))
            .or_else(|| {
                "abcdefghijklmnopqrstuvwxyz0123456789"
                    .chars()
                    .find(|value| !used.contains(value))
            })
            .unwrap_or_default();
        used.insert(chosen);
        row["key"] = Value::String(chosen.to_string());
    }
}

pub(super) fn open_with_applications(facts: &Value) -> Vec<Value> {
    let Some(path) = facts
        .get("paths")
        .and_then(Value::as_array)
        .and_then(|paths| paths.first())
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let mime = facts
        .get("mime")
        .and_then(Value::as_str)
        .unwrap_or_default();
    applications_for(path, mime)
        .into_iter()
        .take(MAX_APPLICATIONS)
        .map(|application| {
            json!({
                "id": "application",
                "desktop_id": application.desktop_id,
                "label": application.name,
                "key": "",
                "glyph": "󰏗",
                "icon": application.icon,
                "icon_source": application.icon_source,
                "icon_mask": application.icon_mask,
                "description": format!("Open with {}", application.name),
                "placements": [],
                "group": "apps",
            })
        })
        .collect::<Vec<_>>()
}

pub(super) fn review_possible(facts: &Value) -> bool {
    facts
        .get("git_root")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
        || facts
            .get("review_pair")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[Value]) -> Vec<&str> {
        rows.iter()
            .filter_map(|row| row["label"].as_str())
            .collect()
    }

    #[test]
    fn application_target_rows_carry_desktop_identity_for_icon_resolution() {
        let target =
            json!({"app":{"desktop_id":"viewer.desktop","name":"Viewer","accepts_files":true}});
        assert_eq!(app_actions(&target)[0]["desktop_id"], "viewer.desktop");
        let target = json!({"editor":{"kind":"nvim","server":"/tmp/nvim.sock"}});
        let facts = json!({"files":["/tmp/example.txt"]});
        assert_eq!(
            editor_actions(&target, &facts)[0]["desktop_id"],
            "nvim.desktop"
        );
    }

    #[test]
    fn herdr_is_one_row_named_after_itself_with_its_placements() {
        let target = json!({"terminal": {"multiplexer": "herdr", "pane_id": "w1:p1"}});
        let facts = json!({"files": ["/tmp/example.txt"], "paths": ["/tmp/example.txt"]});
        let rows = terminal_actions(&target, &facts);
        assert_eq!(labels(&rows), ["herdr"]);
        assert_eq!(rows[0]["id"], "mux-open");
        assert_eq!(rows[0]["icon"], "herdr");
        assert_eq!(
            rows[0]["icon_mask"], "dark",
            "herdr's mark is dark on a light plate, so the plate has to be masked away"
        );
        assert_eq!(
            labels(rows[0]["placements"].as_array().unwrap()),
            ["Vertical split", "Horizontal split", "New tab", "New space"]
        );
        assert_eq!(
            rows[0]["description"],
            "Open in nvim inside a new herdr vertical split, horizontal split, tab, or space"
        );
    }

    #[test]
    fn a_shared_window_without_a_unique_pane_offers_no_mux_or_hunk_placements() {
        let target = json!({"kind": "terminal", "terminal": {"multiplexer": "herdr", "ambiguous": true,
            "reason": "cannot identify this window's pane: its title names no single herdr workspace"}});
        let facts = json!({"files": ["/tmp/a.txt"], "review": true});
        assert!(terminal_actions(&target, &facts).is_empty());
        assert_eq!(resolved_multiplexer(&target), None);
        let rows = actions_for(&target, &facts);
        let review = rows.iter().find(|row| row["id"] == "review");
        assert!(
            review.is_none_or(|row| row["placements"].as_array().is_none_or(|p| p.is_empty())),
            "hunk keeps only its new-window form: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row["id"] == "terminal"),
            "the new-terminal action stays"
        );
        assert_eq!(
            ambiguous_reason(&target).as_deref(),
            Some("cannot identify this window's pane: its title names no single herdr workspace")
        );
    }

    #[test]
    fn tmux_is_one_row_named_after_itself_with_its_placements() {
        let target = json!({"terminal": {"multiplexer": "tmux", "session": "wheeltest"}});
        let facts = json!({"files": ["/tmp/example.txt"], "paths": ["/tmp/example.txt"]});
        let rows = terminal_actions(&target, &facts);
        assert_eq!(labels(&rows), ["tmux"]);
        assert_eq!(rows[0]["icon"], "tmux");
        assert_eq!(
            rows[0]["icon_mask"], "ink",
            "tmux's mark is a dark screen, so the wheel tints its silhouette even when active"
        );
        assert_eq!(
            labels(rows[0]["placements"].as_array().unwrap()),
            [
                "Vertical split",
                "Horizontal split",
                "New window",
                "New session"
            ]
        );
        let keys: Vec<&str> = rows[0]["placements"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["key"].as_str())
            .collect();
        assert_eq!(keys, ["v", "h", "w", "s"]);
        assert_eq!(rows[0]["placements"][0]["icon"], "pane-vertical");
        assert_eq!(rows[0]["placements"][1]["icon"], "pane-horizontal");
    }

    #[test]
    fn a_folder_dropped_on_a_multiplexer_opens_a_shell() {
        let target = json!({"terminal": {"multiplexer": "herdr", "pane_id": "w1:p1"}});
        let facts = json!({"files": [], "directories": ["/tmp/folder"], "paths": ["/tmp/folder"]});
        let rows = terminal_actions(&target, &facts);
        assert_eq!(
            rows[0]["description"],
            "Open a shell in the folder inside a new herdr vertical split, horizontal split, tab, or space"
        );
    }

    #[test]
    fn a_plain_terminal_adds_nothing_of_its_own() {
        let target = json!({"terminal": {"multiplexer": "none"}});
        let facts = json!({"files": ["/tmp/AGENTS.md"], "paths": ["/tmp/AGENTS.md"]});
        assert!(terminal_actions(&target, &facts).is_empty());
        let unresolved = json!({"terminal": {"multiplexer": "herdr", "pane_id": ""}});
        assert!(terminal_actions(&unresolved, &facts).is_empty());
    }

    #[test]
    fn the_wheel_over_herdr_reads_in_the_agreed_order() {
        let target =
            json!({"kind": "terminal", "terminal": {"multiplexer": "herdr", "pane_id": "w1:p1"}});
        let facts = json!({
            "files": ["/tmp/repo/README.md"],
            "paths": ["/tmp/repo/README.md"],
            "directories": [],
            "git_root": "/tmp/repo",
            "mime": "text/plain",
        });
        let rows = actions_for(&target, &facts);
        assert_eq!(
            labels(&rows),
            [
                "herdr",
                "Open in new window",
                "Open with",
                "New terminal",
                "Review with hunk"
            ]
        );
        let keys: Vec<&str> = rows.iter().filter_map(|row| row["key"].as_str()).collect();
        assert_eq!(keys, ["h", "o", "w", "t", "r"]);
        assert!(
            rows.iter().all(|row| row["id"] != "edit"),
            "editing is what opening a text file already does"
        );
    }

    #[test]
    fn tmux_keeps_its_letter_and_the_terminal_row_steps_aside() {
        let target =
            json!({"kind": "terminal", "terminal": {"multiplexer": "tmux", "session": "s"}});
        let facts = json!({"files": ["/tmp/a.txt"], "paths": ["/tmp/a.txt"], "directories": [], "mime": "text/plain"});
        let rows = actions_for(&target, &facts);
        let keys: Vec<&str> = rows.iter().filter_map(|row| row["key"].as_str()).collect();
        assert_eq!(keys, ["t", "o", "w", "n"]);
    }

    #[test]
    fn generic_wheel_has_no_copy_paths_action() {
        let facts = json!({"files": [], "paths": []});
        assert!(
            generic_actions(&facts)
                .iter()
                .all(|row| row.get("id").and_then(Value::as_str) != Some("copy-paths"))
        );
    }

    #[test]
    fn generic_folder_open_names_the_fileblade_route() {
        let facts = json!({"files": [], "directories": ["/tmp/folder"], "paths": ["/tmp/folder"]});
        let rows = generic_actions(&facts);
        assert_eq!(rows[0]["label"], "Open in FileBlade");
        assert_eq!(rows[0]["description"], "Browse the folder in FileBlade");
        assert_eq!(
            labels(&rows),
            ["Open in FileBlade", "Open with", "New terminal"]
        );
    }
}
