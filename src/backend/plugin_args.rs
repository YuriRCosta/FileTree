use super::*;

#[derive(Clone, Debug, Args)]
pub struct ModuleDirsArgs {
    #[arg(long)]
    pub module: String,
}

#[derive(Clone, Debug, Args)]
pub struct ActionListArgs {
    #[arg(long, action = ArgAction::Append)]
    pub provider: Vec<String>,
    #[arg(long, default_value = "")]
    pub user: String,
}

#[derive(Clone, Debug, Args)]
pub struct ActionRunArgs {
    #[arg(long, value_parser = ["plugin", "user"])]
    pub source: String,
    #[arg(long, default_value = "")]
    pub plugin: String,
    #[arg(long, default_value = "")]
    pub plugin_dir: String,
    #[arg(long)]
    pub action: String,
    #[arg(long, value_parser = ["file", "dir", "selection", "root", "none"])]
    pub context: String,
    #[arg(long, default_value = "")]
    pub root: String,
    #[arg(long, action = ArgAction::Append)]
    pub path: Vec<String>,
    #[arg(long)]
    pub yes: bool,
    #[arg(long, default_value = "")]
    pub screen: String,
}
