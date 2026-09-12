use fileblade::migration::{DocumentKind, preflight_document};
use serde_json::json;

#[test]
fn preserves_aliases_consent_overrides_and_unknown_fields() {
    for (kind, value) in [
        (
            DocumentKind::State,
            json!({"version":12,"welcomeDismissed":true,"trashRetentionConsent":"never","future":{"keep":true}}),
        ),
        (
            DocumentKind::Layout,
            json!({"version":1,"blades":{"left":{"open":true,"width":380,"mode":"docked","slots":[{"modules":[{"module":"data-goblin.fileblade-skills/skills","state":{"future":"keep"}},{"module":"data-goblin.goblins/goblins"}],"active":1}]}},"future":42}),
        ),
        (
            DocumentKind::Settings,
            json!({"version":1,"filebladeVersion":"0.1.2","agentManagement":true,"trashRetentionDays":null,"future":{"enabled":false}}),
        ),
        (
            DocumentKind::Keybindings,
            json!({"version":1,"bindings":{"show":"SUPER, E"},"future":{"owner":"user"}}),
        ),
    ] {
        assert_eq!(
            preflight_document(kind, &serde_json::to_vec(&value).unwrap()).unwrap(),
            value
        );
    }
}

#[test]
fn malformed_newer_unversioned_and_invalid_consent_are_refused() {
    for (kind, text, reason) in [
        (DocumentKind::State, "{", "malformed"),
        (DocumentKind::State, r#"{"version":13}"#, "newer schema"),
        (DocumentKind::State, r#"{"version":11}"#, "older schema"),
        (DocumentKind::State, "{}", "missing or invalid"),
        (
            DocumentKind::Layout,
            r#"{"version":1,"blades":[]}"#,
            "blades object",
        ),
        (
            DocumentKind::Settings,
            r#"{"version":1,"agentManagement":"true"}"#,
            "consent",
        ),
        (
            DocumentKind::Settings,
            r#"{"version":1,"trashRetentionDays":3651}"#,
            "retention",
        ),
        (
            DocumentKind::Keybindings,
            r#"{"version":1,"bindings":[]}"#,
            "bindings",
        ),
    ] {
        let error = preflight_document(kind, text.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{error}");
        assert!(error.contains("preserved"), "{error}");
    }
}

#[test]
fn byte_and_nesting_bounds_apply_before_any_import() {
    let kind = DocumentKind::Settings;
    let mut document = br#"{"version":1}"#.to_vec();
    document.resize(kind.limit(), b' ');
    assert!(preflight_document(kind, &document).is_ok());
    document.push(b' ');
    assert!(
        preflight_document(kind, &document)
            .unwrap_err()
            .to_string()
            .contains("byte bound")
    );
    let nested = format!(
        "{{\"version\":1,\"future\":{}0{}}}",
        "[".repeat(256),
        "]".repeat(256)
    );
    assert!(preflight_document(kind, nested.as_bytes()).is_err());
}
