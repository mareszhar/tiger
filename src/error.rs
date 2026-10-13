//! Errors that stop a command. Every error says what happened, the context
//! that locates it, and the most likely fix (see docs/language.md §3).

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Error {
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error { message: message.into(), context: Vec::new(), hint: None }
    }

    pub fn context(mut self, line: impl Into<String>) -> Self {
        self.context.push(line.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// An I/O failure on a path: `could not <action> <path>` with the OS reason.
    pub fn io(action: &str, path: &Path, err: std::io::Error) -> Self {
        Error::new(format!("could not {action} {}", path.display())).context(format!("cause: {err}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tiger: {}", self.message)?;
        for line in &self.context {
            write!(f, "\n  {line}")?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

/// Attaches a path and an action to an `io::Result`.
pub trait IoContext<T> {
    fn at(self, action: &str, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, action: &str, path: &Path) -> Result<T> {
        self.map_err(|err| Error::io(action, path, err))
    }
}
