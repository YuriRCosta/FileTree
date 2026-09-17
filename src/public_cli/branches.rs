use super::*;

#[derive(Clone, Debug, Subcommand)]
pub enum BranchesCommand {
    /// Remove the Branches module from the left blade.
    Close,
    /// Print every branch and worktree of the current directory's repository.
    List,
}

pub(super) fn branches(action: Option<BranchesCommand>) -> AppResult<PublicResult> {
    match action {
        None => simple_ipc("openBranches", &[]),
        Some(BranchesCommand::Close) => simple_ipc("closeBranches", &[]),
        Some(BranchesCommand::List) => list(),
    }
}

fn list() -> AppResult<PublicResult> {
    let document = backend_json(&[
        "git-places".to_string(),
        "--path".to_string(),
        path_text(&std::env::current_dir()?),
    ])?;
    if !document["ok"].as_bool().unwrap_or(false) {
        return Err(AppError::command(error_text(
            &document,
            "branch list is unavailable",
        )));
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    let worktrees = document["worktrees"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut lines = document["branches"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|branch| {
            let checked_out = worktrees
                .iter()
                .find(|worktree| worktree["path"] == branch["worktree"]);
            format!(
                "{} {} {} {} {}",
                if branch["current"].as_bool().unwrap_or(false) {
                    "*"
                } else {
                    " "
                },
                value_text(branch, "name"),
                value_text(branch, "kind"),
                checked_out.map_or_else(|| branch_status(branch), worktree_status),
                relative_time(now - value_i64(branch, "at"))
            )
        })
        .collect::<Vec<_>>();
    lines.extend(worktrees.iter().map(|worktree| {
        format!(
            "{} {} {}",
            value_text(worktree, "path"),
            value_text(worktree, "branch"),
            worktree_status(worktree)
        )
    }));
    Ok(PublicResult::lines(lines, document))
}

fn branch_status(branch: &Value) -> String {
    let (ahead, behind) = (value_i64(branch, "ahead"), value_i64(branch, "behind"));
    if branch["gone"].as_bool().unwrap_or(false) {
        "gone".to_string()
    } else if value_text(branch, "kind") == "remote" {
        "remote".to_string()
    } else if value_text(branch, "upstream").is_empty() {
        "no upstream".to_string()
    } else if ahead == 0 && behind == 0 {
        "in sync".to_string()
    } else {
        [(ahead, "↑"), (behind, "↓")]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, arrow)| format!("{arrow}{count}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn worktree_status(worktree: &Value) -> String {
    let mut parts = Vec::new();
    if value_i64(worktree, "conflicted") > 0 {
        parts.push("conflicts".to_string());
    }
    for (key, label) in [
        ("staged", "staged"),
        ("unstaged", "changed"),
        ("untracked", "untracked"),
    ] {
        let count = value_i64(worktree, key);
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    if parts.is_empty() {
        parts.push("clean".to_string());
    }
    if worktree["locked"].as_bool().unwrap_or(false) {
        parts.push("locked".to_string());
    }
    parts.join(", ")
}

fn relative_time(seconds: i64) -> String {
    let units = [
        (31_536_000, "y"),
        (2_592_000, "mo"),
        (604_800, "w"),
        (86_400, "d"),
        (3600, "h"),
        (60, "m"),
    ];
    units
        .into_iter()
        .find(|(size, _)| seconds >= *size)
        .map_or_else(
            || "just now".to_string(),
            |(size, unit)| format!("{}{unit} ago", seconds / size),
        )
}
