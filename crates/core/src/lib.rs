//! Zeidex core: which formats a file can become, which external engine does it, and how to
//! run that engine. std-only so it builds offline; the GUI and the CLI are thin layers on top.

pub mod actions;
pub mod b64;
pub mod dehaze;
pub mod faces;
pub mod format;
pub mod ico;
pub mod integrate;
pub mod ops;
pub mod pdf;
pub mod plan;
pub mod rename;
pub mod runner;
pub mod selection;
pub mod sha256;
pub mod subtitle;
pub mod tools;
pub mod watch;

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Unsupported(String),
    BadOption(String),
    MissingTool(tools::Tool),
    Failed { tool: String, detail: String },
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
            Error::BadOption(m) => write!(f, "bad option: {m}"),
            Error::MissingTool(t) => write!(f, "{} not found. Install: {}", t.label(), t.install_hint()),
            Error::Failed { tool, detail } => write!(f, "{tool} failed: {detail}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
