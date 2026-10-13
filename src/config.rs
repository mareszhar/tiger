//! The project config, `tiger.toml`: finding it, loading it, validating it.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use crate::error::{Error, IoContext, Result};
use crate::git::is_full_sha;
use crate::util::closest;

pub const FILE: &str = "tiger.toml";
pub const DEFAULT_DIR: &str = "tiger";

const TOP_KEYS: &[&str] = &["dir", "propagate", "refs"];
const REF_KEYS: &[&str] = &["url", "ref", "include", "exclude", "history", "releases", "propagate"];

/// Names in the refs folder that tiger owns besides ref folders; ids may not take them.
pub const RESERVED_IDS: &[&str] = &["TIGER.md"];

#[derive(Debug, Clone)]
pub struct Config {
    pub dir: String,
    pub refs: Vec<RefEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefEntry {
    pub id: String,
    pub url: String,
    /// The branch, tag, or commit as written; `None` follows the default branch.
    pub git_ref: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub history: bool,
    pub releases: bool,
    /// Resolved: the entry's own `propagate`, else the config's, else `true`.
    pub propagate: bool,
}

/// A loaded project: its root (the folder holding `tiger.toml`) and config.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
}

impl Project {
    pub fn config_path(&self) -> PathBuf {
        self.root.join(FILE)
    }

    pub fn dir_path(&self) -> PathBuf {
        self.root.join(&self.config.dir)
    }

    pub fn load(root: &Path) -> Result<Project> {
        let path = root.join(FILE);
        let text = std::fs::read_to_string(&path).at("read", &path)?;
        let config = parse(&text).map_err(|e| e.context(format!("in {}", path.display())))?;
        Ok(Project { root: root.to_path_buf(), config })
    }
}

/// The nearest folder at or above `start` that holds a `tiger.toml`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|dir| dir.join(FILE).is_file()).map(Path::to_path_buf)
}

pub fn parse(text: &str) -> Result<Config> {
    let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        let message = e.to_string();
        Error::new("tiger.toml is not valid TOML").context(message.trim().replace('\n', "\n  "))
    })?;
    let mut problems = Problems::default();

    for (key, _) in doc.iter() {
        if !TOP_KEYS.contains(&key) {
            problems.unknown(key, key, TOP_KEYS);
        }
    }

    let dir = match doc.get("dir") {
        None => DEFAULT_DIR.to_string(),
        Some(item) => match item.as_str() {
            Some(dir) => {
                if let Err(reason) = validate_dir(dir) {
                    problems.push(format!("dir: {reason}"));
                }
                dir.trim_end_matches('/').to_string()
            }
            None => {
                problems.push("dir: must be a string, such as \"tiger\"");
                DEFAULT_DIR.to_string()
            }
        },
    };

    let default_propagate = match doc.get("propagate") {
        None => true,
        Some(item) => item.as_bool().unwrap_or_else(|| {
            problems.push("propagate: must be true or false");
            true
        }),
    };

    let mut refs = Vec::new();
    match doc.get("refs") {
        None => {}
        Some(item) => match item.as_table_like() {
            None => problems.push("refs: must be a table of refs, such as [refs]"),
            Some(table) => {
                for (id, item) in table.iter() {
                    if let Some(entry) = parse_entry(id, item, default_propagate, &mut problems) {
                        refs.push(entry);
                    }
                }
            }
        },
    }

    let mut seen = std::collections::HashSet::new();
    for entry in &refs {
        if !seen.insert(entry.id.to_ascii_lowercase()) {
            problems.push(format!("refs.{}: another id differs only in case; folders would collide", entry.id));
        }
    }

    problems.finish()?;
    Ok(Config { dir, refs })
}

fn parse_entry(id: &str, item: &Item, default_propagate: bool, problems: &mut Problems) -> Option<RefEntry> {
    let at = format!("refs.{id}");
    if let Err(reason) = validate_id(id) {
        problems.push(format!("{at}: {reason}"));
        return None;
    }
    let mut entry = RefEntry {
        id: id.to_string(),
        url: String::new(),
        git_ref: None,
        include: Vec::new(),
        exclude: Vec::new(),
        history: false,
        releases: false,
        propagate: default_propagate,
    };

    if let Some(url) = item.as_str() {
        entry.url = url.trim().to_string();
    } else if let Some(table) = item.as_table_like() {
        for (key, value) in table.iter() {
            let field = format!("{at}.{key}");
            match key {
                "url" => match value.as_str() {
                    Some(url) => entry.url = url.trim().to_string(),
                    None => problems.push(format!("{field}: must be a string")),
                },
                "ref" => match value.as_str() {
                    Some(r) => entry.git_ref = Some(r.trim().to_string()),
                    None => problems.push(format!("{field}: must be a string (a branch, tag, or commit)")),
                },
                "include" | "exclude" => match string_list(value) {
                    Some(list) => {
                        for glob in &list {
                            if let Err(err) = globset::Glob::new(glob) {
                                problems.push(format!("{field}: invalid glob {glob:?}: {}", err.kind()));
                            }
                        }
                        if key == "include" { entry.include = list } else { entry.exclude = list }
                    }
                    None => problems.push(format!("{field}: must be a list of globs, such as [\"src/**\"]")),
                },
                "history" | "releases" | "propagate" => match value.as_bool() {
                    Some(flag) => match key {
                        "history" => entry.history = flag,
                        "releases" => entry.releases = flag,
                        _ => entry.propagate = flag,
                    },
                    None => problems.push(format!("{field}: must be true or false")),
                },
                other => problems.unknown(&field, other, REF_KEYS),
            }
        }
    } else {
        problems.push(format!("{at}: must be a URL string or a table with a url"));
        return None;
    }

    if entry.url.is_empty() {
        problems.push(format!("{at}: needs a url"));
    }
    if let Some(r) = &entry.git_ref {
        if r.is_empty() {
            problems.push(format!("{at}.ref: is empty; remove it to follow the default branch"));
        } else if r.len() < 40 && r.len() >= 7 && r.chars().all(|c| c.is_ascii_hexdigit()) && r.chars().any(|c| c.is_ascii_digit()) {
            problems.push(format!("{at}.ref: {r:?} looks like a short commit SHA; write the full 40-character SHA"));
        } else if is_full_sha(r) {
            entry.git_ref = Some(r.to_ascii_lowercase());
        }
    }
    if entry.history && !(entry.include.is_empty() && entry.exclude.is_empty()) {
        problems.push(format!("{at}: include and exclude don't apply to history = true refs, which are full clones"));
    }
    Some(entry)
}

fn string_list(item: &Item) -> Option<Vec<String>> {
    let array = item.as_array()?;
    array.iter().map(|v: &Value| v.as_str().map(str::to_string)).collect()
}

pub fn validate_id(id: &str) -> std::result::Result<(), String> {
    let valid_chars = id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if id.is_empty() || !valid_chars || !id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("{id:?} is not a valid id; use letters, digits, '.', '_' or '-', starting with a letter or digit"));
    }
    if RESERVED_IDS.contains(&id) || id.ends_with(".releases.md") {
        return Err(format!("{id:?} is reserved by tiger"));
    }
    Ok(())
}

pub fn validate_dir(dir: &str) -> std::result::Result<(), String> {
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() || dir == "." {
        return Err("must name a folder inside the project, such as \"tiger\"".into());
    }
    if Path::new(dir).is_absolute() {
        return Err(format!("{dir:?} must be relative to the project root"));
    }
    for part in dir.split('/') {
        if part == ".." {
            return Err(format!("{dir:?} must stay inside the project"));
        }
        if part.starts_with('.') {
            return Err(format!("{dir:?} must not be hidden: search tools skip folders starting with '.'"));
        }
    }
    Ok(())
}

#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn push(&mut self, problem: impl Into<String>) {
        self.0.push(problem.into());
    }

    fn unknown(&mut self, at: &str, key: &str, valid: &[&str]) {
        let suggestion = closest(key, valid).map(|c| format!("; did you mean '{c}'?")).unwrap_or_default();
        self.0.push(format!("{at}: unknown key '{key}'{suggestion} (valid: {})", valid.join(", ")));
    }

    fn finish(self) -> Result<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        let mut err = Error::new(if self.0.len() == 1 {
            "invalid tiger.toml".to_string()
        } else {
            format!("invalid tiger.toml ({} problems)", self.0.len())
        });
        for problem in self.0 {
            err = err.context(problem);
        }
        Err(err.hint("fix the entries above; `tiger --help` describes every field"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_entry_forms() {
        let config = parse(
            r#"
dir = "refs"
propagate = false

[refs]
# a note
tiptap = "https://github.com/ueberdosis/tiptap"
zod = { url = "https://github.com/colinhacks/zod", ref = "v3.4.2", propagate = true }

[refs.big]
url = "https://github.com/org/big"
include = ["src/**"]
"#,
        )
        .unwrap();
        assert_eq!(config.dir, "refs");
        let ids: Vec<_> = config.refs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["tiptap", "zod", "big"]);
        assert!(!config.refs[0].propagate);
        assert!(config.refs[1].propagate);
        assert_eq!(config.refs[1].git_ref.as_deref(), Some("v3.4.2"));
        assert_eq!(config.refs[2].include, ["src/**"]);
    }

    #[test]
    fn reports_every_problem_with_hints() {
        let err = parse(
            r#"
dirr = "x"
[refs.a]
url = "https://x/a"
inlcude = ["src"]
[refs.b]
url = "https://x/b"
ref = "abc1234"
history = true
exclude = ["*.md"]
"#,
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("unknown key 'dirr'; did you mean 'dir'?"), "{text}");
        assert!(text.contains("refs.a.inlcude: unknown key 'inlcude'; did you mean 'include'?"), "{text}");
        assert!(text.contains("short commit SHA"), "{text}");
        assert!(text.contains("don't apply to history"), "{text}");
    }

    #[test]
    fn rejects_hidden_and_escaping_dirs() {
        assert!(validate_dir(".refs").is_err());
        assert!(validate_dir("../refs").is_err());
        assert!(validate_dir("/abs").is_err());
        assert!(validate_dir("docs/refs").is_ok());
        assert!(validate_id("TIGER.md").is_err());
        assert!(validate_id(".x").is_err());
        assert!(validate_id("pm-model").is_ok());
    }
}
