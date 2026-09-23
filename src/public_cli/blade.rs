use super::*;
use crate::backend::value_name;

pub(super) fn blade(action: BladeCommand) -> AppResult<PublicResult> {
    let response = match action {
        BladeCommand::Focus => ipc("focusBlade", &[String::new()])?,
        BladeCommand::ToggleFocus => ipc("toggleBladeFocus", &[String::new()])?,
        BladeCommand::Open => ipc("openBlade", &[String::new()])?,
        BladeCommand::Close => ipc("closeBlade", &[String::new()])?,
        BladeCommand::Toggle => ipc("toggleBlade", &[String::new()])?,
        BladeCommand::Settings => ipc("toggleBladeSettings", &[String::new()])?,
        BladeCommand::Side(value) => ipc("setBladeSide", &[value_name(value.edge)])?,
        BladeCommand::Width(value) => {
            ipc("setBladeWidth", &[String::new(), value.pixels.to_string()])?
        }
        BladeCommand::Release => ipc("releaseBladeFocus", &[])?,
    };
    Ok(PublicResult::one(response))
}

#[derive(Clone, Debug, Subcommand)]
pub enum BladeCommand {
    Focus,
    ToggleFocus,
    Open,
    Close,
    Toggle,
    Settings,
    Side(EdgeArg),
    Width(BladeWidthArgs),
    Release,
}

#[derive(Clone, Debug, Args)]
pub struct EdgeArg {
    #[arg(value_enum)]
    pub edge: Edge,
}

#[derive(Clone, Debug, Args)]
pub struct BladeWidthArgs {
    #[arg(allow_hyphen_values = true)]
    pub pixels: i64,
}
