use super::*;

pub fn focus_window(address: &str) -> Value {
    let outcome = || -> AppResult<Value> {
        let selector = window_selector(address)?;
        let clients = clients()?;
        let Some(client) = client_by_address(&clients, address) else {
            return Ok(
                json!({"ok": true, "focused": false, "address": address, "reason": "window is gone"}),
            );
        };
        if !field_bool(client, "mapped", true) {
            return Ok(
                json!({"ok": true, "focused": false, "address": address, "reason": "window is gone"}),
            );
        }
        let visible =
            visible_workspace_for_monitor(&monitors_list()?, field_i64(client, "monitor"))
                .unwrap_or(field_i64(&hypr_query("activeworkspace")?, "id"));
        if client_workspace_id(client) != visible {
            return Ok(
                json!({"ok": true, "focused": false, "address": address, "reason": "window is not on its monitor's visible workspace"}),
            );
        }
        if field_str(&active_window(), "address") == address {
            return Ok(
                json!({"ok": true, "focused": true, "address": address, "reason": "already focused"}),
            );
        }
        let before = cursor_position()?;
        hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
        let restored = restore_cursor(before)?;
        Ok(json!({"ok": true, "focused": true, "address": address, "cursorRestored": restored}))
    };
    outcome().unwrap_or_else(|error| {
        json!({"ok": false, "focused": false, "address": address, "error": error.to_string()})
    })
}

pub(super) fn restore_cursor(before: (i64, i64)) -> AppResult<bool> {
    let after = cursor_position()?;
    let moved = (after.0 - before.0).abs() + (after.1 - before.1).abs() > CURSOR_WARP_THRESHOLD;
    if moved {
        hypr_dispatch(&format!(
            "hl.dsp.cursor.move({{ x = {}, y = {} }})",
            before.0, before.1
        ))?;
    }
    Ok(moved)
}

fn check_cancelled(cancelled: &AtomicBool) -> AppResult<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn place_blade_window(options: &PlaceBladeOptions, cancelled: &AtomicBool) -> Value {
    let edge = normalized_edge(&options.edge);
    let outcome = || -> AppResult<Value> {
        check_cancelled(cancelled)?;
        let Some(mut client) = wait_for_blade_client(&options.title, options.timeout, cancelled)?
        else {
            return Ok(json!({
                "ok": false,
                "edge": edge,
                "title": options.title,
                "error": "The blade window did not map in time",
            }));
        };
        let address = field_str(&client, "address");
        let selector = window_selector(&address)?;
        client = root_blade_column(client, &edge, cancelled)?;
        if options.width > 0 {
            client = resize_edge_column(&address, &edge, options.width)?;
        }
        check_cancelled(cancelled)?;
        hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
        let current = clients()?;
        let final_client = client_by_address(&current, &address).unwrap_or(&client);
        Ok(json!({
            "ok": true,
            "edge": edge,
            "title": options.title,
            "address": address,
            "workspace": client_workspace_id(final_client),
            "at": final_client.get("at").cloned().unwrap_or_else(|| json!([])),
            "size": final_client.get("size").cloned().unwrap_or_else(|| json!([])),
            "at_edge": at_edge(final_client, &current, &edge),
        }))
    };
    outcome().unwrap_or_else(|error| {
        json!({
            "ok": false,
            "edge": edge,
            "title": options.title,
            "error": error.to_string(),
        })
    })
}

pub fn focus_direction(options: &FocusDirectionOptions) -> Value {
    let direction = if options.direction.to_lowercase().starts_with('r') {
        "r"
    } else {
        "l"
    };
    let outcome = || -> AppResult<Value> {
        let current = clients()?;
        if options.empty_only {
            return focus_empty_workspace(direction, options, &current);
        }
        if matches!(options.from_blade.as_str(), "left" | "right") {
            return focus_from_blade(direction, options, &current);
        }
        focus_from_window(direction, options, &current)
    };
    outcome()
        .unwrap_or_else(|error| json!({"ok": false, "action": "error", "error": error.to_string()}))
}

pub fn window_dispatch(action: &str, x: i64, y: i64, direction: &str) -> Value {
    let expression = match action {
        "close" => "hl.dsp.window.close()".to_string(),
        "float" => "hl.dsp.window.float({ action = \"toggle\" })".to_string(),
        "resize" => format!("hl.dsp.window.resize({{ x = {x}, y = {y}, relative = true }})"),
        "swap" => format!(
            "hl.dsp.window.swap({{ direction = \"{}\" }})",
            direction_letter(direction)
        ),
        "focus" => format!(
            "hl.dsp.focus({{ direction = \"{}\" }})",
            direction_letter(direction)
        ),
        _ => return json!({"ok": false, "action": action, "error": "unknown action"}),
    };
    match hypr_dispatch(&expression) {
        Ok(()) => json!({"ok": true, "action": action}),
        Err(error) => json!({"ok": false, "action": action, "error": error.to_string()}),
    }
}

pub fn window_selector(address: &str) -> AppResult<String> {
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let pattern = ADDRESS.get_or_init(|| Regex::new(r"^0x[0-9a-fA-F]+$").expect("address regex"));
    if pattern.is_match(address) {
        Ok(format!("address:{address}"))
    } else {
        Err(AppError::invalid(format!(
            "Invalid Hyprland window address: {address}"
        )))
    }
}

pub(super) fn tiled_clients(clients: &[Value], workspace_id: i64) -> Vec<&Value> {
    clients
        .iter()
        .filter(|client| {
            client_workspace_id(client) == workspace_id
                && field_bool(client, "mapped", true)
                && !field_bool(client, "floating", false)
        })
        .collect()
}

pub(super) fn edge_client<'a>(
    clients: &'a [Value],
    workspace_id: i64,
    edge: &str,
) -> Option<&'a Value> {
    let tiled = tiled_clients(clients, workspace_id);
    if edge == "right" {
        tiled.into_iter().max_by_key(|client| {
            let (x, _, width, _) = client_rect(client);
            x + width
        })
    } else {
        tiled.into_iter().min_by_key(|client| client_rect(client).0)
    }
}

pub(super) fn at_edge(client: &Value, clients: &[Value], edge: &str) -> bool {
    let (x, _, width, _) = client_rect(client);
    tiled_clients(clients, client_workspace_id(client))
        .into_iter()
        .all(|candidate| {
            let (candidate_x, _, candidate_width, _) = client_rect(candidate);
            if edge == "right" {
                candidate_x + candidate_width <= x + width
            } else {
                candidate_x >= x
            }
        })
}

pub(super) fn full_height(client: &Value, clients: &[Value]) -> bool {
    let tiled = tiled_clients(clients, client_workspace_id(client));
    let Some(top) = tiled.iter().map(|candidate| client_rect(candidate).1).min() else {
        return true;
    };
    let bottom = tiled
        .iter()
        .map(|candidate| {
            let (_, y, _, height) = client_rect(candidate);
            y + height
        })
        .max()
        .unwrap_or(top);
    let (_, y, _, height) = client_rect(client);
    y <= top + 8 && y + height >= bottom - 8
}

pub(super) fn has_neighbour(client: &Value, clients: &[Value], direction: &str) -> bool {
    let (x, y, width, height) = client_rect(client);
    let address = field_str(client, "address");
    tiled_clients(clients, client_workspace_id(client))
        .into_iter()
        .filter(|candidate| field_str(candidate, "address") != address)
        .any(|candidate| {
            let (candidate_x, candidate_y, candidate_width, candidate_height) =
                client_rect(candidate);
            let overlaps = candidate_y < y + height && candidate_y + candidate_height > y;
            overlaps
                && if direction == "l" {
                    candidate_x + candidate_width <= x
                } else {
                    candidate_x >= x + width
                }
        })
}

pub(super) fn prepare_tiled_client(
    client: &Value,
    workspace_id: i64,
    selector: &str,
) -> AppResult<()> {
    if client_workspace_id(client) != workspace_id {
        hypr_dispatch(&format!(
            "hl.dsp.window.move({{ workspace = \"{workspace_id}\", follow = false, window = \"{selector}\" }})"
        ))?;
        thread::sleep(Duration::from_millis(60));
    }
    if field_bool(client, "floating", false) {
        hypr_dispatch(&format!(
            "hl.dsp.window.float({{ action = \"disable\", window = \"{selector}\" }})"
        ))?;
        thread::sleep(Duration::from_millis(60));
    }
    Ok(())
}

pub(super) fn swap_client_to_edge(
    address: &str,
    edge: &str,
    selector: &str,
    mut client: Value,
) -> AppResult<Value> {
    let direction = if edge == "right" { "r" } else { "l" };
    for _ in 0..32 {
        let current = clients()?;
        client = client_by_address(&current, address)
            .cloned()
            .ok_or_else(|| {
                AppError::command("The new window disappeared while it was being placed")
            })?;
        if at_edge(&client, &current, edge) {
            break;
        }
        let before_x = client_rect(&client).0;
        hypr_dispatch(&format!(
            "hl.dsp.window.swap({{ direction = \"{direction}\", window = \"{selector}\" }})"
        ))?;
        thread::sleep(Duration::from_millis(50));
        let updated = clients()?;
        let Some(candidate) = client_by_address(&updated, address) else {
            break;
        };
        let after_x = client_rect(candidate).0;
        if (direction == "l" && after_x >= before_x) || (direction == "r" && after_x <= before_x) {
            break;
        }
    }
    Ok(client_by_address(&clients()?, address)
        .cloned()
        .unwrap_or(client))
}

pub(super) fn place_window(address: &str, workspace_id: i64, edge: &str) -> AppResult<Value> {
    let selector = window_selector(address)?;
    let current = clients()?;
    let client = client_by_address(&current, address)
        .cloned()
        .ok_or_else(|| AppError::command("The new window disappeared before it could be placed"))?;
    prepare_tiled_client(&client, workspace_id, &selector)?;
    let client = swap_client_to_edge(address, edge, &selector, client)?;
    hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
    Ok(json!({
        "address": address,
        "workspace": client_workspace_id(&client),
        "at": client.get("at").cloned().unwrap_or_else(|| json!([])),
        "size": client.get("size").cloned().unwrap_or_else(|| json!([])),
        "class": field_str(&client, "class"),
        "title": field_str(&client, "title"),
    }))
}

pub(super) fn wait_for_blade_client(
    title: &str,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> AppResult<Option<Value>> {
    let deadline =
        Instant::now() + timeout.clamp(Duration::from_millis(200), Duration::from_secs(20));
    while Instant::now() < deadline {
        check_cancelled(cancelled)?;
        let chosen = clients()?
            .into_iter()
            .filter(|client| {
                field_str(client, "title") == title && field_bool(client, "mapped", true)
            })
            .max_by_key(|client| field_str(client, "address"));
        if chosen.is_some() {
            return Ok(chosen);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(None)
}

pub(super) fn root_blade_column(
    mut client: Value,
    edge: &str,
    cancelled: &AtomicBool,
) -> AppResult<Value> {
    let address = field_str(&client, "address");
    let selector = window_selector(&address)?;
    if field_bool(&client, "floating", false) {
        hypr_dispatch(&format!(
            "hl.dsp.window.float({{ action = \"disable\", window = \"{selector}\" }})"
        ))?;
        thread::sleep(Duration::from_millis(60));
    }
    hypr_dispatch(&format!("hl.dsp.layout(\"movetoroot {selector} stable\")"))?;
    thread::sleep(Duration::from_millis(60));
    let mut current = clients()?;
    client = client_by_address(&current, &address)
        .cloned()
        .unwrap_or(client);
    if !full_height(&client, &current) {
        check_cancelled(cancelled)?;
        hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
        thread::sleep(Duration::from_millis(40));
        hypr_dispatch("hl.dsp.layout(\"togglesplit\")")?;
        thread::sleep(Duration::from_millis(60));
        current = clients()?;
        client = client_by_address(&current, &address)
            .cloned()
            .unwrap_or(client);
    }
    if !at_edge(&client, &current, edge) {
        check_cancelled(cancelled)?;
        hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
        thread::sleep(Duration::from_millis(40));
        hypr_dispatch("hl.dsp.layout(\"swapsplit\")")?;
        thread::sleep(Duration::from_millis(60));
        current = clients()?;
        client = client_by_address(&current, &address)
            .cloned()
            .unwrap_or(client);
    }
    if !at_edge(&client, &current, edge) {
        check_cancelled(cancelled)?;
        prepare_tiled_client(&client, client_workspace_id(&client), &selector)?;
        client = swap_client_to_edge(&address, edge, &selector, client)?;
    }
    Ok(client)
}

pub(super) fn resize_edge_column(address: &str, edge: &str, target_width: i64) -> AppResult<Value> {
    let selector = window_selector(address)?;
    let current = clients()?;
    let mut client = client_by_address(&current, address)
        .cloned()
        .ok_or_else(|| {
            AppError::command("The blade window disappeared before it could be resized")
        })?;
    let mut sign = if edge == "right" { -1 } else { 1 };
    for _ in 0..2 {
        let delta = target_width - client_rect(&client).2;
        if delta.abs() <= 4 {
            break;
        }
        hypr_dispatch(&format!(
            "hl.dsp.window.resize({{ window = \"{selector}\", x = {}, y = 0, relative = true }})",
            sign * delta
        ))?;
        thread::sleep(Duration::from_millis(120));
        let updated = clients()?;
        let Some(candidate) = client_by_address(&updated, address) else {
            break;
        };
        client = candidate.clone();
        if (client_rect(&client).2 - target_width).abs() <= 4 {
            break;
        }
        sign = -sign;
    }
    Ok(client)
}

pub(super) fn focus_from_blade(
    direction: &str,
    options: &FocusDirectionOptions,
    current: &[Value],
) -> AppResult<Value> {
    let from_blade = &options.from_blade;
    let toward_workspace =
        (from_blade == "left" && direction == "r") || (from_blade == "right" && direction == "l");
    if !toward_workspace {
        return Ok(json!({
            "ok": true,
            "action": "none",
            "reason": format!("nothing beyond the {from_blade} blade"),
        }));
    }
    let workspace = focused_workspace()?;
    let Some(target) = edge_client(current, field_i64(&workspace, "id"), from_blade) else {
        let (target_edge, target_state) = direction_target(direction, options);
        let edge = if target_state == "open" {
            target_edge
        } else {
            from_blade.as_str()
        };
        let monitor = field_str(&workspace, "monitor");
        if !options.blade_reachable(edge, &monitor) {
            return Ok(json!({
                "ok": true,
                "action": "none",
                "reason": format!("the {edge} blade lives on another monitor"),
            }));
        }
        return Ok(json!({
            "ok": true,
            "action": "focus-blade",
            "edge": edge,
            "monitor": monitor,
            "reason": "no tiled window on the workspace",
        }));
    };
    let address = field_str(target, "address");
    let selector = window_selector(&address)?;
    hypr_dispatch(&format!("hl.dsp.focus({{ window = \"{selector}\" }})"))?;
    Ok(json!({
        "ok": true,
        "action": "focus-window",
        "address": address,
        "class": field_str(target, "class"),
    }))
}

pub(super) fn focus_from_window(
    direction: &str,
    options: &FocusDirectionOptions,
    current: &[Value],
) -> AppResult<Value> {
    let active = hypr_query("activewindow")?;
    let active_address = field_str(&active, "address");
    let client = client_by_address(current, &active_address);
    let blade_window = client.is_some_and(|value| {
        options
            .blade_titles
            .iter()
            .any(|title| title == &field_str(value, "title"))
    });
    let edge = if direction == "r" { "right" } else { "left" };
    let state = if edge == "right" {
        &options.right_state
    } else {
        &options.left_state
    };
    if active_address.is_empty() {
        let fallback = focus_empty_workspace(direction, options, current)?;
        if fallback.get("action").and_then(Value::as_str) == Some("focus-blade") {
            return Ok(fallback);
        }
    }
    if let Some(client) = client
        && !blade_window
        && !field_bool(client, "floating", false)
        && !has_neighbour(client, current, direction)
        && state == "open"
    {
        let monitor = monitor_name(client.get("monitor").cloned().unwrap_or(Value::Null));
        if !options.blade_reachable(edge, &monitor) {
            hypr_dispatch(&format!("hl.dsp.focus({{ direction = \"{direction}\" }})"))?;
            return Ok(
                json!({"ok": true, "action": "dispatched", "direction": direction, "reason": format!("the {edge} blade lives on another monitor")}),
            );
        }
        return Ok(json!({
            "ok": true,
            "action": "focus-blade",
            "edge": edge,
            "monitor": monitor,
            "address": active_address,
        }));
    }
    hypr_dispatch(&format!("hl.dsp.focus({{ direction = \"{direction}\" }})"))?;
    Ok(json!({"ok": true, "action": "dispatched", "direction": direction}))
}

pub(super) fn focus_empty_workspace(
    direction: &str,
    options: &FocusDirectionOptions,
    current: &[Value],
) -> AppResult<Value> {
    let workspace = focused_workspace()?;
    let workspace_id = field_i64(&workspace, "id");
    if current.iter().any(|client| {
        field_bool(client, "mapped", true) && client_workspace_id(client) == workspace_id
    }) {
        return Ok(json!({
            "ok": true,
            "action": "none",
            "reason": "the workspace still has a window",
        }));
    }
    let (edge, state) = direction_target(direction, options);
    if state != "open" {
        return Ok(json!({
            "ok": true,
            "action": "none",
            "reason": format!("the {edge} blade is not docked and open"),
        }));
    }
    let monitor = field_str(&workspace, "monitor");
    if !options.blade_reachable(edge, &monitor) {
        return Ok(json!({
            "ok": true,
            "action": "none",
            "reason": format!("the {edge} blade lives on another monitor"),
        }));
    }
    Ok(json!({
        "ok": true,
        "action": "focus-blade",
        "edge": edge,
        "monitor": monitor,
        "reason": "the workspace has no window",
    }))
}

pub(super) fn focused_workspace() -> AppResult<Value> {
    let monitors = monitors_list()?;
    if let Some(monitor) = monitors
        .iter()
        .find(|monitor| field_bool(monitor, "focused", false))
    {
        return Ok(json!({
            "id": visible_workspace_of(monitor),
            "monitor": field_str(monitor, "name"),
        }));
    }
    hypr_query("activeworkspace")
}

pub(super) fn direction_target<'a>(
    direction: &str,
    options: &'a FocusDirectionOptions,
) -> (&'static str, &'a str) {
    if direction == "r" {
        ("right", &options.right_state)
    } else {
        ("left", &options.left_state)
    }
}

pub(super) fn direction_letter(direction: &str) -> char {
    direction
        .to_lowercase()
        .chars()
        .next()
        .filter(|value| matches!(value, 'l' | 'r' | 'u' | 'd'))
        .unwrap_or('l')
}

pub(super) fn normalized_edge(value: &str) -> String {
    if value == "right" { "right" } else { "left" }.to_string()
}
