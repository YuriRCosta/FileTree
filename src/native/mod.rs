use clap::{Args as ClapArgs, Subcommand};
use fileblade_output::Output;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::ExitCode;
use std::sync::Arc;

mod backend;

#[derive(Clone, Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Debug, Subcommand)]
enum Command {
    Portal,
    Backend {
        #[arg(last = true, num_args = 1..)]
        arguments: Vec<OsString>,
    },
    Ipc {
        #[arg(last = true, num_args = 2..)]
        arguments: Vec<OsString>,
    },
}

pub fn run(args: Args, output: Arc<Output>) -> ExitCode {
    let result = match args.command {
        Command::Backend { arguments } => match backend::run(arguments, Arc::clone(&output)) {
            Ok(true) => return ExitCode::SUCCESS,
            Ok(false) => return ExitCode::FAILURE,
            Err(error) => Err(error),
        },
        Command::Portal => crate::chooser::portal::serve(),
        Command::Ipc { arguments } => crate::paths::app_root().and_then(|root| {
            Err(std::process::Command::new("qs")
                .args(["ipc", "-n", "-p"])
                .arg(root.join("app"))
                .args(["call", "--"])
                .args(arguments)
                .exec()
                .into())
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = output.error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}
