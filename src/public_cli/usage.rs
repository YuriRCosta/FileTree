use super::*;
use crate::module_helpers::CoreRoute;

#[derive(Clone, Debug, Subcommand)]
pub enum UsageCommand {
    /// Print skill uses per local day, read from agent transcripts.
    Skills,
    /// Print MCP calls per local day, read from agent transcripts.
    Mcp,
    /// Delete recorded skill and MCP use history; transcripts already read are not imported again.
    Forget(UsageForgetArgs),
}

#[derive(Clone, Debug, Args)]
pub struct UsageForgetArgs {
    /// Delete only events on local days before YYYY-MM-DD; omit to delete every event.
    #[arg(long, value_parser = local_day)]
    pub before: Option<String>,
}

fn local_day(value: &str) -> Result<String, String> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|day| day.format("%Y-%m-%d").to_string() == value)
        .map(|_| value.to_string())
        .ok_or_else(|| "expected a calendar date as YYYY-MM-DD".to_string())
}

pub(super) fn usage(action: UsageCommand) -> AppResult<PublicResult> {
    let (route, command, method, mut arguments) = match action {
        UsageCommand::Skills => (
            CoreRoute::Skills,
            "helper-read",
            "usage",
            vec![
                "--project".to_string(),
                path_text(&std::env::current_dir()?),
            ],
        ),
        UsageCommand::Mcp => (CoreRoute::Mcp, "helper-read", "usage", Vec::new()),
        UsageCommand::Forget(options) => (
            CoreRoute::Mcp,
            "helper-write",
            "usage-forget",
            options
                .before
                .map(|day| vec!["--before".to_string(), day])
                .unwrap_or_default(),
        ),
    };
    arguments.push("--json".to_string());
    let document = backend_json(&[
        command.to_string(),
        "--provider".to_string(),
        route.provider().to_string(),
        "--plugin-dir".to_string(),
        String::new(),
        "--helper".to_string(),
        "inventory".to_string(),
        "--method".to_string(),
        method.to_string(),
        "--arguments".to_string(),
        serde_json::to_string(&arguments)?,
    ])?;
    if !document["ok"].as_bool().unwrap_or(false) {
        return Err(AppError::command(error_text(
            &document,
            "usage history is unavailable",
        )));
    }
    let lines = if method == "usage-forget" {
        vec![format!("removed {}", value_i64(&document, "removed"))]
    } else {
        document["days"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|day| format!("{}\t{}", day[0].as_str().unwrap_or(""), day[1]))
            .collect()
    };
    Ok(PublicResult::lines(lines, document))
}
