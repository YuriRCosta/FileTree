use std::{fs, path::Path};

#[test]
fn right_click_keeps_the_menu_inside_the_blade_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface =
        fs::read_to_string(root.join("blades/BladeSurface.qml")).expect("read BladeSurface.qml");
    let menu = fs::read_to_string(root.join("panes/FileActionsMenu.qml"))
        .expect("read FileActionsMenu.qml");
    let stack =
        fs::read_to_string(root.join("blades/BladeStack.qml")).expect("read BladeStack.qml");
    let files_module =
        fs::read_to_string(root.join("modules/files/Module.qml")).expect("read files Module.qml");

    assert!(surface.contains("onActiveChanged: surface.host.pointerHeld = active"));
    assert!(!surface.contains("keyboardFocusForced"));
    assert!(menu.contains("Popup {"));
    assert!(menu.contains("parent: hostWindow ? hostWindow.contentItem : null"));
    assert!(menu.contains("visible: requestedVisible"));
    assert!(menu.contains("ownerEdge === requestedEdge"));
    assert!(menu.contains("hostWindow.surfaceActive !== false"));
    assert!(stack.contains("PluginPanes.FileActionsMenu {"));
    assert!(!files_module.contains("FileActionsMenu {"));
    assert!(menu.contains("readonly property bool hostActive: hostWindow && hostWindow.contentItem ? hostWindow.contentItem.Window.active : true"));
    assert!(
        menu.contains(
            "onHostActiveChanged: if (!hostActive && visible) controller.closeActionMenu()"
        )
    );
    assert!(!menu.contains("WlrLayershell.keyboardFocus"));
    assert!(!menu.contains("PopupWindow"));
    assert!(!menu.contains("Timer {"));
    assert!(!menu.contains("HyprlandFocusGrab"));
    assert!(menu.contains("controller.focusTree(null, true)"));
    assert!(surface.contains("property alias actionKeys: surfaceKeys"));
    assert!(menu.contains("shared: root.hostWindow ? root.hostWindow.actionKeys : null"));
    assert!(menu.contains("if (!root.actionKeys.isRepeat(event)) root.submitInput()"));
    assert!(menu.contains("actionKeys: root.actionKeys"));

    assert!(
        surface
            .contains("mask: liveWidth > 0 ? null : (actionMenuHere ? actionMenuMask : sheetMask)")
    );
    assert!(surface.contains("id: actionMenuLayer"));
    assert!(surface.contains("function handleFocusGrabCleared()"));
    assert!(surface.contains("onCleared: surface.handleFocusGrabCleared()"));
    assert!(surface.contains(
        "if (sheetHover.hovered || stack.modulePickerOpen || surface.actionMenuOpen || surface.dropWheelOpen) return"
    ));
    assert!(surface.contains("onHoveredChanged: if (!hovered && surface.actionMenuOpen)"));
    assert!(surface.contains("surface.host.actionMenuPointerExited()"));
    assert!(menu.contains(
        "onHoveredChanged: if (!hovered) root.hostWindow.host.actionMenuPointerExited()"
    ));

    let focus = fs::read_to_string(root.join("blades/BladeFocusController.qml"))
        .expect("read BladeFocusController.qml");
    assert!(focus.contains(
        "host.dragActive || host.pressActive || host.pointerHeld || host.resizeActive || menuOpen"
    ));
    let focus_blade = focus
        .split("function focusBlade(")
        .nth(1)
        .and_then(|body| body.split("function focusSlot(").next())
        .expect("focusBlade body");
    assert!(focus_blade.contains("focusedEdge = target"));
    let report_focus = focus
        .split("function reportFocus(")
        .nth(1)
        .and_then(|body| body.split("onFocusedEdgeChanged:").next())
        .expect("reportFocus body");
    assert!(report_focus.contains("if (!isWindowMode(target)) return"));
    assert!(focus.contains("function bladePointerExited(edge, screen)"));
    assert!(focus.contains("if (!pointerBusy && hoverExitPending) hoverExitTimer.restart()"));
    assert!(focus.contains("if (controller.pointerBusy)"));
    assert!(focus.contains("function actionMenuPointerExited()"));
    assert!(focus.contains("!controller.actionMenuLayerContains(parsed.x, parsed.y)"));

    let hyprland = fs::read_dir(root.join("src/hyprland"))
        .expect("read src/hyprland")
        .flatten()
        .map(|entry| fs::read_to_string(entry.path()).expect("read hyprland module"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(hyprland.contains("pub fn hover_target(blade_titles: &[String])"));
    assert!(!hyprland.contains("thread::sleep(base_interval"));
}
