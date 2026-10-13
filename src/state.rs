//! `.tiger-state.json`: what tiger last put in a project's refs folder.
//! It lives in the gitignored refs folder, so syncing never touches the
//! project's git status.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::RefEntry;
use crate::copy::CopyMode;
use crate::error::{Error, IoContext, Result};
use crate::remote::Kind;

pub const FILE: &str = ".tiger-state.json";
pub const LOCK: &str = ".tiger-lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub refs: BTreeMap<String, RefState>,
}

impl Default for State {
    fn default() -> Self {
        State { version: 1, refs: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RefState {
    pub url: String,
    /// The `ref` field as configured when synced; `None` followed the default branch.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub kind: Kind,
    /// The branch or tag name, or the commit SHA.
    pub name: String,
    pub commit: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub history: bool,
    pub copy_mode: CopyMode,
    /// Digest of the remote's tag list when release notes were last written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags_digest: Option<String>,
    /// When the copy was last made.
    pub synced_at: u64,
    /// When a sync last confirmed the copy matches upstream (or its pin).
    #[serde(default)]
    pub checked_at: u64,
}

impl RefState {
    /// Whether the copy was made from the same config the entry has now
    /// (release notes aside, which never change the copy).
    pub fn matches(&self, entry: &RefEntry) -> bool {
        self.url == entry.url
            && self.git_ref == entry.git_ref
            && self.include == entry.include
            && self.exclude == entry.exclude
            && self.history == entry.history
    }
}

pub fn path(refs_dir: &Path) -> PathBuf {
    refs_dir.join(FILE)
}

pub fn load(refs_dir: &Path) -> Result<State> {
    let path = path(refs_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
        Err(err) => return Err(Error::io("read", &path, err)),
    };
    serde_json::from_str(&text).map_err(|err| {
        Error::new(format!("could not read {}", path.display()))
            .context(format!("cause: {err}"))
            .hint("delete the state file and the ref folders it listed, then run `tiger sync`")
    })
}

pub fn save(refs_dir: &Path, state: &State) -> Result<()> {
    let path = path(refs_dir);
    let mut text = serde_json::to_string_pretty(state).expect("state serializes");
    text.push('\n');
    crate::util::write_atomic(&path, text.as_bytes())
}

/// Takes the project's sync lock, creating the refs folder if needed.
pub fn lock(refs_dir: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(refs_dir).at("create", refs_dir)?;
    let path = refs_dir.join(LOCK);
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).at("open", &path)?;
    file.lock().at("lock", &path)?;
    Ok(file)
}
