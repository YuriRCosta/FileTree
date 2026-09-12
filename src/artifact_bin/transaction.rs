use super::*;
use crate::module_helpers::{self, Route};

pub fn remove_with_helper(
    module: &str,
    raw_item: &str,
    raw_route: &str,
    arguments: &str,
    cancelled: &AtomicBool,
) -> Value {
    match prepare_and_remove(module, raw_item, raw_route, arguments, cancelled) {
        Ok(response) => response,
        Err(error) => refusal(error, ""),
    }
}

fn parse_route(raw: &str) -> Result<Route, String> {
    if raw.len() > MAX_ITEM_JSON {
        return Err("recovery helper route is too large".to_string());
    }
    serde_json::from_str(raw).map_err(|_| "invalid recovery helper route".to_string())
}

fn prepare_and_remove(
    module: &str,
    raw_item: &str,
    raw_route: &str,
    arguments: &str,
    cancelled: &AtomicBool,
) -> Result<Value, String> {
    if !valid_module(module) {
        return Err("invalid bin module name".to_string());
    }
    let parsed = parse_item(raw_item)?;
    if !parsed.paths.is_empty() || !parsed.payload.is_null() {
        return Err(
            "helper recovery needs metadata without paths or a supplied payload".to_string(),
        );
    }
    let route = parse_route(raw_route)?;
    for (method, write) in [
        ("prepare-remove", true),
        ("remove-prepared", true),
        ("restore", true),
        ("discard", true),
    ] {
        route
            .validate(method, write)
            .map_err(|error| error.to_string())?;
    }
    let _lease = mutation_lease()?;
    let transaction_id = uuid::Uuid::new_v4().simple().to_string();
    let mut prepare_args =
        module_helpers::validate_arguments(arguments).map_err(|error| error.to_string())?;
    prepare_args.extend(["--transaction-id".to_string(), transaction_id.clone()]);
    let arguments = serde_json::to_string(&prepare_args).map_err(|error| error.to_string())?;
    let root = bin_root(module).map_err(|error| error.to_string())?;
    let entry_dir = prepare_entry(&root)?;
    let entry_id = format!(
        "{BIN_PREFIX}{}",
        entry_dir.file_name().unwrap().to_string_lossy()
    );
    let mut manifest = manifest_for(module, &parsed, Vec::new());
    manifest.restore_helper = Some(route.clone());
    manifest.helper_record_id = Some(transaction_id.clone());
    if let Err(error) = store_items(&entry_dir, &manifest, cancelled) {
        let _ = discard_entry(&entry_dir, None);
        return Err(format!(
            "could not save recovery; source was not changed: {error}"
        ));
    }
    let preparation = (|| -> Result<(String, String), String> {
        let prepared = module_helpers::run(
            &route.request("prepare-remove", &arguments, None, true),
            cancelled,
        )
        .map_err(|error| error.to_string())?;
        if prepared["ok"] != true
            || prepared["schemaVersion"] != SCHEMA_VERSION
            || !prepared["payload"].is_object()
            || prepared["recordId"] != transaction_id
        {
            return Err(format!(
                "helper could not prepare recovery: {}",
                prepared["message"]
                    .as_str()
                    .unwrap_or("incomplete recovery record")
            ));
        }
        let payload = prepared["payload"].clone();
        let input = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
        if input.len() > module_helpers::INPUT_LIMIT {
            return Err(
                "recovery payload exceeds the helper input limit; source was not changed".into(),
            );
        }
        let mut complete: Value =
            serde_json::from_str(raw_item).map_err(|error| error.to_string())?;
        complete["payload"] = payload.clone();
        complete["restoreHelper"] =
            serde_json::to_value(&route).map_err(|error| error.to_string())?;
        parse_item(&serde_json::to_string(&complete).map_err(|error| error.to_string())?)
            .map_err(|_| "complete recovery record exceeds 64 KiB; source was not changed")?;
        prepare_args.push("--payload-stdin".into());
        let commit_args =
            serde_json::to_string(&prepare_args).map_err(|error| error.to_string())?;
        module_helpers::validate_arguments(&commit_args).map_err(|error| error.to_string())?;
        manifest.payload = payload;
        save_manifest(&entry_dir, &manifest).map_err(|error| error.to_string())?;
        Ok((input, commit_args))
    })();
    let (input, commit_args) = match preparation {
        Ok(prepared) => prepared,
        Err(error) => {
            return Ok(match discard_entry(&entry_dir, Some(&manifest)) {
                Ok(()) => refusal(error, ""),
                Err(cleanup) => helper_failure(
                    &Value::Null,
                    &format!("{error}; recovery remains in the bin: {cleanup}"),
                    &entry_id,
                ),
            });
        }
    };
    let removed = module_helpers::run(
        &route.request("remove-prepared", &commit_args, Some(&input), true),
        cancelled,
    );
    match removed {
        Ok(response) if response["ok"] == true && response["schemaVersion"] == SCHEMA_VERSION => {
            Ok(document(
                vec![result("", true, true, "disabled; recovery saved")],
                &entry_id,
                Value::Null,
            ))
        }
        Ok(response) => Ok(helper_failure(
            &response,
            "Removal was not confirmed; recovery is saved. Use Restore to reconcile the source",
            &entry_id,
        )),
        Err(error) => Ok(helper_failure(
            &Value::Null,
            &format!(
                "Removal was not confirmed; recovery is saved: {error}. Use Restore to reconcile the source"
            ),
            &entry_id,
        )),
    }
}

pub fn restore_with_helper(
    module: &str,
    entry_id: &str,
    raw_route: Option<&str>,
    cancelled: &AtomicBool,
) -> Value {
    if matches!(module, "skills" | "memory")
        && let Err(error) = crate::preferences::require_agent_management()
    {
        return refusal(error.to_string(), "");
    }
    let helper = match raw_route.map(parse_route).transpose() {
        Ok(helper) => helper,
        Err(error) => return refusal(error, ""),
    };
    let _lease = match mutation_lease() {
        Ok(lease) => lease,
        Err(error) => return refusal(error, ""),
    };
    restore_entry(module, entry_id, helper, cancelled)
}

fn same_recovery_route(left: &Route, right: &Route) -> bool {
    if left.provider == right.provider && left.helper == right.helper {
        return true;
    }
    match (
        module_helpers::canonical_route(&left.provider, &left.helper),
        module_helpers::canonical_route(&right.provider, &right.helper),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

pub(super) fn restore_payload(
    entry_dir: &Path,
    entry_id: &str,
    manifest: &mut Manifest,
    helper: Option<Route>,
    cancelled: &AtomicBool,
) -> Value {
    let route = match (helper, manifest.restore_helper.as_ref()) {
        (Some(route), Some(saved)) if !same_recovery_route(&route, saved) => {
            return refusal(
                "the recovery helper does not match this record's provider",
                "",
            );
        }
        (Some(route), _) => route,
        (None, Some(route)) => route.clone(),
        (None, None) => return refusal("this recovery record needs its owning companion", ""),
    };
    if manifest.helper_restored {
        return match discard_entry(entry_dir, Some(manifest)) {
            Ok(()) => document(
                vec![result(
                    "",
                    true,
                    false,
                    "already restored; recovery cleaned",
                )],
                entry_id,
                Value::Null,
            ),
            Err(error) => refusal(error, entry_id),
        };
    }
    let input = match serde_json::to_string(&manifest.payload) {
        Ok(input) if input.len() <= module_helpers::INPUT_LIMIT => input,
        _ => {
            return refusal(
                "recovery payload exceeds the helper input limit; the record was kept",
                "",
            );
        }
    };
    if let Err(error) = route.validate("restore", true) {
        return refusal(
            format!("Recovery helper unavailable; the record was kept: {error}"),
            "",
        );
    }
    if manifest
        .restore_helper
        .as_ref()
        .is_none_or(|saved| saved.provider == route.provider && saved.helper == route.helper)
    {
        manifest.restore_helper = Some(route.clone());
    }
    if let Err(error) = save_manifest(entry_dir, manifest) {
        return refusal(format!("Could not save the recovery helper: {error}"), "");
    }
    let record_id = manifest.helper_record_id.as_deref().unwrap_or(entry_id);
    let use_payload = !manifest.payload.is_null();
    let arguments = if use_payload {
        json!(["--record-id", record_id, "--payload-stdin", "--json"])
    } else {
        json!(["--record-id", record_id, "--json"])
    }
    .to_string();
    let response = module_helpers::run(
        &route.request(
            "restore",
            &arguments,
            use_payload.then_some(input.as_str()),
            true,
        ),
        cancelled,
    );
    match response {
        Ok(response) if response["ok"] == true && response["schemaVersion"] == SCHEMA_VERSION => {
            manifest.helper_restored = true;
            if let Err(error) = save_manifest(entry_dir, manifest) {
                return refusal(
                    format!(
                        "Source restored but completion could not be saved; retry Restore: {error}"
                    ),
                    entry_id,
                );
            }
            if let Err(error) = discard_entry(entry_dir, Some(manifest)) {
                return helper_failure(
                    &Value::Null,
                    &format!("Source restored but recovery cleanup failed; retry Restore: {error}"),
                    entry_id,
                );
            }
            document(
                vec![result("", true, true, "restored")],
                entry_id,
                Value::Null,
            )
        }
        Ok(response) => helper_failure(
            &response,
            "Restore was not confirmed; the recovery record was kept",
            entry_id,
        ),
        Err(error) => helper_failure(
            &Value::Null,
            &format!("Restore was not confirmed; the recovery record was kept: {error}"),
            entry_id,
        ),
    }
}

fn helper_failure(response: &Value, fallback: &str, entry: &str) -> Value {
    let detail = response
        .get("message")
        .or_else(|| response.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = if detail.is_empty() {
        fallback.to_string()
    } else {
        format!("{fallback}: {}", truncate(detail, 200))
    };
    document(vec![result("", false, false, message)], entry, Value::Null)
}

pub(super) fn discard_helper_record(entry_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let Some(route) = &manifest.restore_helper else {
        return Ok(());
    };
    if manifest.helper_record_id.is_none() {
        let parent = entry_dir.parent().ok_or("bin entry has no parent")?;
        let (names, truncated) =
            secure::directory_names_bounded(parent, 1000).map_err(|error| error.to_string())?;
        if truncated {
            return Err(
                "cannot establish whether another bin entry needs this recovery record".to_string(),
            );
        }
        for name in names {
            let other = parent.join(name);
            if other == entry_dir {
                continue;
            }
            if let Some((saved, _)) =
                load_manifest(&other, Some(&manifest.module)).map_err(|error| error.to_string())?
                && saved.payload == manifest.payload
                && saved.restore_helper.as_ref().is_some_and(|helper| {
                    helper.provider == route.provider && helper.helper == route.helper
                })
            {
                return Ok(());
            }
        }
    }
    let legacy = manifest.helper_record_id.is_none();
    let input = serde_json::to_string(&manifest.payload).map_err(|error| error.to_string())?;
    let arguments = if let Some(id) = &manifest.helper_record_id {
        json!(["--record-id", id, "--json"])
    } else {
        json!(["--record-id", "legacy", "--payload-stdin", "--json"])
    }
    .to_string();
    let response = module_helpers::run(
        &route.request(
            "discard",
            &arguments,
            legacy.then_some(input.as_str()),
            true,
        ),
        &AtomicBool::new(false),
    )
    .map_err(|error| format!("recovery data was kept: {error}"))?;
    if response["ok"] != true || response["schemaVersion"] != SCHEMA_VERSION {
        return Err(
            "the companion did not confirm recovery cleanup; the bin entry was kept".to_string(),
        );
    }
    Ok(())
}
