use super::*;

pub fn drop_run(options: &DropRunOptions) -> Value {
    let facts = file_facts(&options.paths);
    let mut runner = Runner::new(options.dry_run);
    let target = parse_target(&options.target_json);
    if facts.get("count").and_then(Value::as_u64).unwrap_or(0) == 0 {
        return json!({
            "ok": false,
            "error": "Nothing to drop",
            "action": options.action,
            "commands": [],
        });
    }
    if let Err(error) = revalidate_target(&target) {
        return json!({
            "ok": false,
            "error": error.to_string(),
            "action": options.action,
            "placement": options.placement,
            "commands": [],
            "dry_run": options.dry_run,
        });
    }
    let result = if options.action == "application" {
        run_application(&mut runner, &options.desktop_id, &facts)
    } else if options.action == "review" {
        run_review(&mut runner, &options.placement, &target, &facts)
    } else if matches!(
        options.action.as_str(),
        "open" | "review" | "terminal" | "copy-paths"
    ) {
        run_generic(&mut runner, &options.action, &facts)
    } else {
        run_target(
            &mut runner,
            &options.action,
            &options.placement,
            &target,
            &facts,
        )
    };
    extend_result(
        result.unwrap_or_else(|error| json!({"ok": false, "error": error.to_string()})),
        json!({
            "action": options.action,
            "placement": options.placement,
            "commands": runner.commands,
            "dry_run": options.dry_run,
        }),
    )
}

pub fn drop_paste(options: &DropPasteOptions) -> Value {
    let context = drop_context(options.x, options.y, &options.paths, &options.blade_titles);
    if !context.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return json!({
            "ok": false,
            "error": context.get("error").and_then(Value::as_str).unwrap_or("Could not resolve the drop target"),
            "form": options.form,
            "commands": [],
        });
    }
    let mut runner = Runner::new(options.dry_run);
    let target = context
        .get("target")
        .cloned()
        .unwrap_or_else(|| json!({"kind": "desktop"}));
    let facts = context.get("files").cloned().unwrap_or_else(|| json!({}));
    let result = run_paste(&mut runner, &target, &facts, &options.form)
        .unwrap_or_else(|error| json!({"ok": false, "error": error.to_string()}));
    extend_result(
        result,
        json!({
            "form": options.form,
            "target": target.get("label").and_then(Value::as_str).unwrap_or_default(),
            "commands": runner.commands,
            "dry_run": options.dry_run,
        }),
    )
}

pub(super) fn run_target(
    runner: &mut Runner,
    action: &str,
    placement: &str,
    target: &Value,
    facts: &Value,
) -> AppResult<Value> {
    if let Some(reason) = ambiguous_reason(target) {
        return Ok(
            json!({"ok": false, "error": format!("{reason}; open a new terminal window instead")}),
        );
    }
    if action == "mux-open" {
        if let Err(reason) = revalidate_title_target(target) {
            return Ok(
                json!({"ok": false, "error": format!("{reason}; open a new terminal window instead")}),
            );
        }
        let result = run_multiplexer(runner, placement, target, facts)?;
        if result.get("ok").and_then(Value::as_bool) == Some(true) {
            focus_target(runner, target)?;
        }
        return Ok(result);
    }
    match action {
        "nvim-open" => run_nvim(runner, placement, target, facts),
        "app-open" => {
            let result = run_application(
                runner,
                target
                    .get("app")
                    .and_then(|app| app.get("desktop_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                facts,
            )?;
            if result.get("ok").and_then(Value::as_bool) == Some(true) {
                focus_target(runner, target)?;
            }
            Ok(result)
        }
        _ => Ok(json!({"ok": false, "error": format!("Unknown drop action: {action}")})),
    }
}

pub(super) fn run_generic(runner: &mut Runner, action: &str, facts: &Value) -> AppResult<Value> {
    let paths = string_array(facts, "paths");
    let folder = text_field(facts, "folder");
    match action {
        "open" => {
            for path in paths {
                runner.detached(launch_command(&parse_path(&path)?, "default", "", 0)?, None)?;
            }
            Ok(json!({"ok": true, "error": ""}))
        }
        "review" => {
            let cwd = review_cwd(facts);
            let mut command = vec![
                OsString::from("xdg-terminal-exec"),
                OsString::from("--app-id=org.omarchy.hunk"),
                native_directory_arg(&cwd)?,
                OsString::from("-e"),
            ];
            command.extend(
                review_command(facts)
                    .into_iter()
                    .map(|value| {
                        if value.starts_with("file://") {
                            parse_path(&value).map(PathBuf::into_os_string)
                        } else {
                            Ok(value.into())
                        }
                    })
                    .collect::<std::io::Result<Vec<_>>>()?,
            );
            runner.detached(wrapped(command), None)?;
            Ok(json!({"ok": true, "error": ""}))
        }
        "terminal" => {
            runner.detached(
                wrapped(vec![
                    OsString::from("xdg-terminal-exec"),
                    native_directory_arg(&folder)?,
                ]),
                None,
            )?;
            Ok(json!({"ok": true, "error": ""}))
        }
        "copy-paths" => {
            let (code, _, error) = runner.capture_input(
                vec![
                    "wl-copy".to_string(),
                    "--type".to_string(),
                    "text/plain".to_string(),
                ],
                None,
                Some(quoted_paths(&paths).into_bytes()),
            )?;
            Ok(json!({"ok": code == 0, "error": error.trim()}))
        }
        _ => Ok(json!({"ok": false, "error": format!("Unknown drop action: {action}")})),
    }
}

pub(super) fn run_multiplexer(
    runner: &mut Runner,
    placement: &str,
    target: &Value,
    facts: &Value,
) -> AppResult<Value> {
    run_multiplexer_command(
        runner,
        placement,
        target,
        &text_field(facts, "folder"),
        &multiplexer_command(facts),
    )
}

fn run_review(
    runner: &mut Runner,
    placement: &str,
    target: &Value,
    facts: &Value,
) -> AppResult<Value> {
    if !review_possible(facts) {
        return Ok(
            json!({"ok": false, "error": "Select a Git repository path or two files to review"}),
        );
    }
    if matches!(placement, "" | "window") {
        return run_generic(runner, "review", facts);
    }
    if let Some(reason) = ambiguous_reason(target) {
        return Ok(
            json!({"ok": false, "error": format!("{reason}; open a new terminal window instead")}),
        );
    }
    if let Err(reason) = revalidate_title_target(target) {
        return Ok(
            json!({"ok": false, "error": format!("{reason}; open a new terminal window instead")}),
        );
    }
    let Some(multiplexer) = resolved_multiplexer(target) else {
        return Ok(json!({"ok": false, "error": "This target has no resolved multiplexer"}));
    };
    let destination = match (multiplexer, placement) {
        (_, "pane" | "right" | "down") => placement,
        ("tmux", "tab") => "window",
        ("tmux", "workspace") => "session",
        (_, "tab" | "workspace") => placement,
        _ => return Ok(json!({"ok": false, "error": "Unknown review placement"})),
    };
    let result = run_multiplexer_command(
        runner,
        destination,
        target,
        &review_cwd(facts),
        &review_command(facts),
    )?;
    if result["ok"] == true {
        focus_target(runner, target)?;
    }
    Ok(result)
}

fn run_multiplexer_command(
    runner: &mut Runner,
    placement: &str,
    target: &Value,
    cwd: &str,
    command: &[String],
) -> AppResult<Value> {
    match target
        .get("terminal")
        .and_then(|terminal| terminal.get("multiplexer"))
        .and_then(Value::as_str)
        .unwrap_or("none")
    {
        "herdr" => run_herdr(runner, placement, target, cwd, command),
        "tmux" => run_tmux(runner, placement, target, cwd, command),
        _ => Ok(json!({"ok": false, "error": "This terminal runs no supported multiplexer"})),
    }
}

pub(super) fn run_paste(
    runner: &mut Runner,
    target: &Value,
    facts: &Value,
    form: &str,
) -> AppResult<Value> {
    let text = paste_text(target, facts, form);
    if let Some(reason) = ambiguous_reason(target) {
        return Ok(
            json!({"ok": false, "error": format!("{reason}; open a new terminal window instead"), "text": text}),
        );
    }
    if target.get("kind").and_then(Value::as_str) != Some("terminal")
        && text.chars().any(|character| character.is_control())
    {
        return Ok(json!({
            "ok": false,
            "error": "a path contains control characters; refusing to type it into a window",
            "text": text,
        }));
    }
    let terminal = target.get("terminal").unwrap_or(&Value::Null);
    let target_focused = focus_target(runner, target)?;
    match terminal
        .get("multiplexer")
        .and_then(Value::as_str)
        .unwrap_or("none")
    {
        "herdr" if !text_field(terminal, "pane_id").is_empty() => {
            if let Err(reason) = revalidate_title_target(target) {
                return Ok(json!({"ok": false, "error": reason, "text": text}));
            }
            let (ok, _, error) = herdr_call(
                runner,
                &["pane", "send-text", &text_field(terminal, "pane_id"), &text],
                &text_field(terminal, "socket"),
            )?;
            return Ok(json!({"ok": ok, "error": if ok { "" } else { &error }, "text": text}));
        }
        "tmux" if !text_field(terminal, "session").is_empty() => {
            let mut command = tmux_command(&text_field(terminal, "socket"));
            command.extend([
                "send-keys".to_string(),
                "-t".to_string(),
                text_field(terminal, "session"),
                "-l".to_string(),
                "--".to_string(),
                text.clone(),
            ]);
            let (code, _, error) = runner.capture(native_tmux_arguments(command)?, None)?;
            return Ok(json!({"ok": code == 0, "error": error.trim(), "text": text}));
        }
        _ => {}
    }
    if which("wtype").is_none() && !runner.dry_run {
        return Ok(json!({"ok": false, "error": "wtype is not installed", "text": text}));
    }
    if target_focused {
        runner.pause(Duration::from_millis(80));
    }
    let (code, _, error) = runner.capture(
        vec!["wtype".to_string(), "--".to_string(), text.clone()],
        None,
    )?;
    Ok(json!({"ok": code == 0, "error": error.trim(), "text": text}))
}

pub(super) fn run_application(
    runner: &mut Runner,
    desktop_id: &str,
    facts: &Value,
) -> AppResult<Value> {
    if let Err(error) = validate_desktop_id(desktop_id) {
        return Ok(json!({"ok": false, "error": error.to_string()}));
    }
    let mut command = vec!["gtk-launch".to_string(), desktop_id.to_string()];
    command.extend(application_arguments(desktop_id, facts));
    runner.detached(command, None)?;
    Ok(json!({"ok": true, "error": ""}))
}

pub(super) fn application_arguments(desktop_id: &str, facts: &Value) -> Vec<String> {
    string_array(facts, "paths")
        .into_iter()
        .filter_map(|path| {
            let path = parse_path(&path).ok()?;
            if desktop_id.to_lowercase().starts_with("obsidian") {
                Some(format!("obsidian://open?path={}", encode_path(&path)))
            } else {
                Url::from_file_path(path).ok().map(|url| url.to_string())
            }
        })
        .collect()
}

pub(super) fn paste_text(target: &Value, facts: &Value, form: &str) -> String {
    let base = target
        .get("terminal")
        .and_then(|terminal| terminal.get("cwd"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            facts
                .get("git_root")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| text_field(facts, "folder"));
    let paths = string_array(facts, "paths")
        .into_iter()
        .filter_map(|path| parse_path(&path).ok())
        .map(|path| {
            if form == "relative" && !base.is_empty() {
                parse_path(&base)
                    .ok()
                    .and_then(|base| relative_path(&base, &path))
                    .unwrap_or(path)
            } else {
                path
            }
        })
        .collect::<Vec<_>>();
    if target.get("kind").and_then(Value::as_str) == Some("terminal") {
        format!(
            "{} ",
            paths
                .iter()
                .map(|path| shell_quote_native(path.as_os_str()))
                .collect::<Vec<_>>()
                .join(" ")
        )
    } else {
        paths
            .iter()
            .map(|path| {
                if path.is_absolute() {
                    path_text(path)
                } else {
                    command_text(path.as_os_str())
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_uris_encode_native_paths_once() {
        let facts = json!({"paths": ["file:///fixture/%FF.txt", "/fixture/�.txt"]});
        assert_eq!(
            application_arguments("obsidian.desktop", &facts),
            [
                "obsidian://open?path=/fixture/%FF.txt",
                "obsidian://open?path=/fixture/%EF%BF%BD.txt"
            ]
        );
        assert_eq!(
            application_arguments("org.example.App.desktop", &facts),
            ["file:///fixture/%FF.txt", "file:///fixture/%EF%BF%BD.txt"]
        );
    }

    fn assert_focus_command(command: &[String], address: &str) {
        assert_eq!(command.first().map(String::as_str), Some("hyprctl"));
        assert!(command.iter().any(|part| part.contains(address)));
    }

    #[test]
    fn paste_focuses_the_target_before_typing_or_multiplexer_input() {
        let mut runner = Runner::new(true);
        let target = json!({
            "kind": "terminal",
            "address": "0xabc",
            "terminal": {"multiplexer": "tmux", "session": "drop"}
        });
        let facts = json!({"paths": ["/tmp/one"], "folder": "/tmp"});

        assert_eq!(
            run_paste(&mut runner, &target, &facts, "absolute").unwrap()["ok"],
            true
        );
        assert_focus_command(&runner.commands[0], "0xabc");
        assert!(runner.commands[1].iter().any(|part| part == "send-keys"));
    }

    #[test]
    fn existing_app_actions_finish_by_focusing_their_target() {
        let facts = json!({"paths": ["/tmp/one"], "files": ["/tmp/one"], "folder": "/tmp"});

        let mut nvim = Runner::new(true);
        let editor = json!({"address": "0xdef", "editor": {"server": "/tmp/nvim.sock"}});
        assert_eq!(
            run_nvim(&mut nvim, "buffer", &editor, &facts).unwrap()["ok"],
            true
        );
        assert_eq!(nvim.commands[0][0], "nvim");
        assert_focus_command(&nvim.commands[1], "0xdef");

        let mut app = Runner::new(true);
        let target = json!({"address": "0x123", "app": {"desktop_id": "org.example.App.desktop"}});
        assert_eq!(
            run_target(&mut app, "app-open", "", &target, &facts).unwrap()["ok"],
            true
        );
        assert_eq!(app.commands[0][0], "gtk-launch");
        assert_focus_command(&app.commands[1], "0x123");
    }

    #[test]
    fn multiplexer_actions_focus_the_terminal_after_the_internal_change() {
        let mut runner = Runner::new(true);
        let target = json!({
            "address": "0x456",
            "terminal": {"multiplexer": "tmux", "session": "drop"}
        });
        let facts = json!({"paths": ["/tmp/one"], "files": ["/tmp/one"], "folder": "/tmp"});

        assert_eq!(
            run_target(&mut runner, "mux-open", "pane", &target, &facts).unwrap()["ok"],
            true
        );
        assert!(runner.commands[0].iter().any(|part| part == "split-window"));
        assert!(
            runner.commands[0]
                .iter()
                .all(|part| part != "-h" && part != "-v")
        );
        assert!(runner.commands[0].iter().any(|part| part.contains("nvim")));
        assert_focus_command(&runner.commands[1], "0x456");
    }

    #[test]
    fn tmux_splits_follow_the_chosen_direction() {
        let target = json!({
            "address": "0x456",
            "terminal": {"multiplexer": "tmux", "session": "drop"}
        });
        let facts = json!({"paths": ["/tmp/one"], "files": ["/tmp/one"], "folder": "/tmp"});
        for (placement, flag) in [("right", "-h"), ("down", "-v")] {
            let mut runner = Runner::new(true);
            run_target(&mut runner, "mux-open", placement, &target, &facts).unwrap();
            let split = &runner.commands[0];
            let at = split
                .iter()
                .position(|part| part == "split-window")
                .unwrap();
            assert_eq!(split[at + 1], flag);
        }
    }

    #[test]
    fn herdr_splits_follow_the_chosen_direction_without_asking_for_the_layout() {
        let target = json!({
            "address": "0x456",
            "terminal": {"multiplexer": "herdr", "pane_id": "w1:p1", "workspace_id": "w1"}
        });
        let facts = json!({"paths": ["/tmp/one"], "files": ["/tmp/one"], "folder": "/tmp"});
        for placement in ["right", "down"] {
            let mut runner = Runner::new(true);
            run_target(&mut runner, "mux-open", placement, &target, &facts).unwrap();
            let split = &runner.commands[0];
            assert!(split.iter().any(|part| part == "split"));
            assert!(split.iter().all(|part| part != "layout"));
            let at = split.iter().position(|part| part == "--direction").unwrap();
            assert_eq!(split[at + 1], placement);
        }
    }

    #[test]
    fn retired_wheel_actions_are_refused() {
        let facts = json!({"paths": ["/tmp/one"], "files": ["/tmp/one"], "folder": "/tmp"});
        let terminal =
            json!({"address": "", "terminal": {"multiplexer": "tmux", "session": "drop"}});
        for action in [
            "edit",
            "term-view",
            "term-paste",
            "mux-shell",
            "mux-review",
            "mux-paste",
        ] {
            let mut runner = Runner::new(true);
            let result = if action == "edit" {
                run_generic(&mut runner, action, &facts).unwrap()
            } else {
                run_target(&mut runner, action, "pane", &terminal, &facts).unwrap()
            };
            assert_eq!(result["ok"], false, "{action}");
            assert!(
                runner.commands.is_empty(),
                "{action} ran {:?}",
                runner.commands
            );
        }
    }
}
