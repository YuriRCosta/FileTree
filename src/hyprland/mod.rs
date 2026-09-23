use crate::command::{CommandSpec, which};
use crate::common::{CONTROL_TIMEOUT, own_binary, parse_path, path_error, path_text};
use crate::{AppError, AppResult};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

mod blades;
mod borders;
mod hover;
mod launch;
mod socket;
pub use blades::*;
pub use borders::*;
pub use hover::*;
pub use launch::*;
pub use socket::*;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct FocusDirectionOptions {
    pub direction: String,
    pub left_state: String,
    pub right_state: String,
    pub from_blade: String,
    pub blade_titles: Vec<String>,
    pub empty_only: bool,
    pub left_monitor: String,
    pub right_monitor: String,
}

impl FocusDirectionOptions {
    pub fn blade_reachable(&self, edge: &str, monitor: &str) -> bool {
        let home = if edge == "right" {
            &self.right_monitor
        } else {
            &self.left_monitor
        };
        home.is_empty() || home == monitor
    }
}

#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub path: String,
    pub mode: String,
    pub desktop_id: String,
    pub timeout: Duration,
    pub line: u64,
}

const CURSOR_WARP_THRESHOLD: i64 = 8;
