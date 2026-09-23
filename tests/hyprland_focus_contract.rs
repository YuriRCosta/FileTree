use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use std::{fs, path::Path};

fn source(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn cancelled_blade_placement_stops_before_touching_hyprland() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let result = fileblade::hyprland::place_blade_window(
        &fileblade::hyprland::PlaceBladeOptions {
            title: "cancelled blade".to_string(),
            edge: "left".to_string(),
            width: 380,
            timeout: Duration::from_secs(1),
        },
        &cancelled,
    );

    assert!(cancelled.load(Ordering::Relaxed));
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"], "operation cancelled");
}

#[test]
fn teardown_restores_owned_borders_and_cancels_pending_dim_work() {
    let focus = source("blades/BladeFocusController.qml");
    let destroyed = focus.split("Component.onDestruction:").nth(1).unwrap();
    assert!(destroyed.contains("disposing = true"));
    assert!(destroyed.contains("service.cancelBackendRequest(dimRequestId"));
    assert!(!focus.contains("execDetached"));
    assert!(source("src/server/lifecycle.rs").contains("crate::hyprland::restore_owned_borders()"));
    assert!(source("src/server/input.rs").contains("libc::sigaction(libc::SIGTERM"));
    let borders = source("src/hyprland/borders.rs");
    assert!(borders.find("open_private_lock").unwrap() < borders.find("clients()?").unwrap());
    assert!(borders.contains("same_window(client, address, record)"));
    assert!(borders.contains("record[\"dimmed\"] == value"));
    assert!(borders.contains("record[\"owner\"] != pid"));
    assert!(borders.contains("rustix::process::pidfd_open"));
    assert!(source("controllers/BackendClient.qml").contains("\"--after-exit\", owner"));
}

fn function_body<'a>(qml: &'a str, name: &str, next: &str) -> &'a str {
    qml.split(&format!("function {name}("))
        .nth(1)
        .and_then(|body| body.split(&format!("function {next}(")).next())
        .unwrap_or_else(|| panic!("find {name} body"))
}

#[test]
fn keyboard_focus_remains_grabbed_after_the_deferred_target_focus() {
    let surface = source("blades/BladeSurface.qml");
    let take_focus = function_body(&surface, "takeKeyboardFocus", "handleFocusGrabCleared");
    let focus_slot = function_body(&surface, "focusSlot", "commitLiveWidth");

    assert!(take_focus.contains("keyboardFocusReleased = false"));
    assert!(focus_slot.contains("takeKeyboardFocus()"));
    assert!(focus_slot.contains("Qt.callLater(function()"));
    assert!(focus_slot.contains("if (target) target.takeFocus(part)"));
    assert!(!focus_slot.contains("keyboardFocusReleased = true"));
    assert!(!surface.contains("keyboardFocusForced"));
    assert!(surface.contains("keyboardFocusReleased ? WlrKeyboardFocus.None"));
    assert!(surface.contains(": WlrKeyboardFocus.OnDemand"));
    assert!(!surface.contains("WlrKeyboardFocus.Exclusive"));
}

#[test]
fn outside_pointer_motion_is_compared_with_the_focus_time_baseline() {
    let watcher = source("blades/BladePointerFocusWatch.qml");

    assert!(watcher.contains("baselineReady = false"));
    assert!(watcher.contains("service.backendRequest(\"hover-target\""));
    assert!(watcher.contains("if (!watch.baselineReady)"));
    assert!(watcher.contains(
        "var moved = Math.abs(x - watch.baselineX) + Math.abs(y - watch.baselineY) >= 2"
    ));
    assert!(watcher.contains("if (!moved)"));
    assert!(watcher.contains("parsed.action !== \"hover-focus\""));
    assert!(watcher.contains("watch.controller.pointerBusy"));
    assert!(watcher.contains("watch.controller.releaseFocus(edge)"));
    assert!(watcher.contains("watch.controller.restoreWorkspaceFocus()"));
    assert!(watcher.contains("onFocusedEdgeChanged: begin()"));
}

#[test]
fn focus_grab_yields_outside_but_not_to_an_inside_click_or_menu() {
    let surface = source("blades/BladeSurface.qml");
    let cleared = function_body(&surface, "handleFocusGrabCleared", "slotItem");

    assert!(surface.contains("active: surface.bladeOpen && !surface.keyboardFocusReleased"));
    assert!(surface.contains("windows: [surface]"));
    assert!(surface.contains("onCleared: surface.handleFocusGrabCleared()"));
    assert!(cleared.contains("Qt.callLater(function()"));
    assert!(cleared.contains(
        "if (sheetHover.hovered || stack.modulePickerOpen || surface.actionMenuOpen || surface.dropWheelOpen) return"
    ));
    assert!(cleared.contains("surface.host.releaseFocus(surface.edge, surface.screen, revision)"));
    assert!(surface.contains("id: sheetHover"));
    assert!(surface.contains("onActiveChanged: surface.host.pointerHeld = active"));
}

#[test]
fn deferred_focus_and_grab_clear_are_bound_to_one_monitor_and_request() {
    let surface = source("blades/BladeSurface.qml");
    let controller = source("blades/BladeFocusController.qml");
    let focus_slot = function_body(&surface, "focusSlot", "commitLiveWidth");
    let released = function_body(&surface, "releaseKeyboardFocus", "containsScenePoint");
    let cleared = function_body(&surface, "handleFocusGrabCleared", "followSheetPointer");

    assert!(surface.contains("host.focusMatches(edge, screen, revision)"));
    assert!(surface.contains("targetScreen !== surface.screen"));
    assert!(surface.contains("pendingFocusRevision = host.focusRevision"));
    assert!(released.contains("focusRequestTimer.stop()"));
    assert!(released.contains("pendingFocusRevision = -1"));
    assert!(focus_slot.contains("if (!ownsFocus(revision)) return"));
    assert!(focus_slot.contains("if (!surface.ownsFocus(revision)) return"));
    assert!(cleared.contains("var revision = host.focusRevision"));
    assert!(cleared.contains("!surface.ownsFocus(revision)"));
    assert!(controller.contains("focusedScreen === screen && focusRevision === revision"));
    assert!(
        controller.contains("if (screen && !focusMatches(target, screen, revision)) return false")
    );
    assert!(controller.contains("host.bladeFocusRequested(target, focusedScreen, index"));
    let exited = function_body(&controller, "bladePointerExited", "resolveHoverExit");
    assert!(exited.contains("focusedScreen !== screen"));
    assert!(surface.contains("surface.host.bladePointerExited(surface.edge, surface.screen)"));
    assert!(
        source("blades/BladePointerFocusWatch.qml").contains("onFocusRevisionChanged: begin()")
    );
}

#[test]
fn every_external_launch_yields_blade_focus_without_restoring_the_old_window() {
    let focus = source("blades/BladeFocusController.qml");
    let yielded = function_body(&focus, "yieldFocus", "orderedSlots");
    let host = source("blades/BladeHost.qml");
    let service = source("Service.qml");
    let launcher = source("controllers/LaunchController.qml");
    let surface = source("blades/BladeSurface.qml");
    let placement = source("src/hyprland/blades.rs");
    let navigation = source("controllers/NavigationController.qml");
    let search = source("controllers/SearchController.qml");
    let launch_external = function_body(&launcher, "launchExternal", "finish");
    let menu = source("panes/FileActionsMenu.qml");

    assert!(yielded.contains("emptyWorkspaceFocusTimer.stop()"));
    assert!(yielded.contains("externalFocusHandoff = true"));
    assert!(yielded.contains("externalFocusHandoffTimer.restart()"));
    assert!(yielded.contains("service.cancelBackendRequest(activeWindowRequestId"));
    assert!(yielded.contains("service.cancelBackendRequest(restoreRequestId"));
    assert!(yielded.contains("host.bladeFocusReleased(edges[i])"));
    assert!(yielded.contains("focusedEdge = \"\""));
    assert!(yielded.contains("restoreFocusAddress = \"\""));
    assert!(!yielded.contains("restoreWorkspaceFocus()"));
    assert!(focus.contains("if (!externalFocusHandoff && focusedEdge === \"\""));
    assert!(focus.contains("name === \"openwindow\" || name === \"openwindowv2\""));
    assert!(focus.contains("id: externalFocusHandoffTimer"));
    let host_yield = function_body(&host, "yieldFocus", "bladePointerEntered");
    assert!(host_yield.contains("focusEpoch++"));
    assert!(host_yield.contains("service.cancelBackendRequest(windowFocusRequestId"));
    assert!(host_yield.contains("windowFocusGeneration++"));
    assert!(host_yield.contains("return focusController.yieldFocus()"));
    assert!(surface.contains("focusRequestTimer.stop()"));
    assert!(surface.contains("if (surface.keyboardFocusReleased) return"));
    assert!(surface.contains("surface.pointerRefocusRequired = surface.host.externalFocusHandoff"));
    assert!(surface.contains("sheetHover.hovered ? sheetHover.point.scenePosition.x : -1"));
    assert!(surface.contains("onSheetHoverPositionChanged: followSheetPointer()"));
    assert!(surface.contains("if (!surface.pointerRefocusRequired && !surface.bladeFocused"));
    assert!(placement.contains("check_cancelled(cancelled)?;"));
    assert!(navigation.contains("function yieldFocus() { focusTimer.stop() }"));
    assert!(
        search.contains("function yieldFocus() { focusTimer.stop(); quickNavTargetScreen = null }")
    );
    assert!(service.contains("function yieldFocusForExternalLaunch()"));
    assert!(
        service.contains("if (dropWheelOpen) dropWheelController.keyboardFocusReleased = true")
    );
    assert!(service.contains("return bladeHost.yieldFocus()"));
    assert!(service.contains("navigationController.yieldFocus()"));
    assert!(service.contains("searchController.yieldFocus()"));
    assert!(launch_external.contains("service.yieldFocusForExternalLaunch()"));
    assert!(
        launch_external
            .find("service.yieldFocusForExternalLaunch()")
            .unwrap()
            < launch_external
                .find("service.backendRequest(\"launch\"")
                .unwrap()
    );
    assert!(menu.contains("if (directory === true) root.returnToTree()"));
    let editor_action = menu
        .split("text: \"  Open in LazyVim\"")
        .nth(1)
        .and_then(|body| body.split("text: \"󰆏  Copy\"").next())
        .expect("editor action body");
    assert!(!editor_action.contains("returnToTree()"));
}

#[test]
fn slot_hover_requires_pointer_motion_before_it_changes_keyboard_focus() {
    let slot = source("blades/BladeSlot.qml");
    let follow_pointer = function_body(&slot, "followPointer", "overlayOpen");

    assert!(follow_pointer.contains("if (previousX < 0) return"));
    assert!(follow_pointer.contains(
        "if (Math.abs(lastHoverX - previousX) + Math.abs(lastHoverY - previousY) < 2) return"
    ));
    assert!(follow_pointer.contains("slot.activeFocus && host.focusedEdge === edge"));
    assert!(follow_pointer.contains("if (context.collapsed || ownsFocus"));
    assert!(follow_pointer.contains("host.focusSlot(edge, slotIndex"));
    assert!(slot.contains("slot.lastHoverX = hovered ? point.scenePosition.x : -1"));
}
