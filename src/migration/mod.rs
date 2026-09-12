use std::path::PathBuf;

mod prepare;
mod storage;
pub use prepare::prepare;
mod writer;
pub use writer::{LegacyWriter, legacy_writer};
mod documents;
pub use documents::{DocumentKind, preflight_document};

#[derive(Clone, Debug)]
pub struct Roots {
    pub config: PathBuf,
    pub state: PathBuf,
    pub recovery: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Ready,
    ReadOnly { reason: String },
    Refused { reason: String },
}

#[derive(Clone, Debug)]
pub struct Preparation {
    pub status: Status,
    pub receipt_path: PathBuf,
}
