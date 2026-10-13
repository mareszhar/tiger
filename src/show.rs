//! Introspection, never the network: `tiger show` says what each copy is
//! (commit, date, subject, size, who else uses the repo), and `tiger store`
//! says what the shared store holds and which projects use it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{self, Project, RefEntry};
use crate::copy::CopyMode;
use crate::error::{Error, Result};
use crate::git;
use crate::remote::Kind;
use crate::settings;
use crate::state::{self, RefState};
use crate::store::{FALLBACK_DIR, Store};
use crate::url::store_key;

/// The commit a copy holds, as git describes it.
#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub sha: String,
    /// Committer date, `YYYY-MM-DD`.
    pub date: String,
    pub author: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Elsewhere {
    pub project: String,
    pub id: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefDetails {
    pub id: String,
    pub url: String,
    /// The `ref` field as configured; absent follows the default branch.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub synced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitInfo>,
    /// When the copy was last made (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copied_at: Option<u64>,
    /// When a sync last confirmed it current (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_mode: Option<CopyMode>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    /// Total size of the copy's files, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    pub history: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    /// Other registered projects with a ref to the same repo.
    pub elsewhere: Vec<Elsewhere>,
    /// True when the entry changed since its last sync.
    pub config_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowReport {
    pub project: String,
    pub refs: Vec<RefDetails>,
}

pub fn show(start: &Path, ids: &[String]) -> Result<ShowReport> {
    let root = config::find_root(start).ok_or_else(|| {
        Error::new(format!("no {} in {} or any parent folder", config::FILE, start.display())).hint("run `tiger init` in the project root")
    })?;
    let project = Project::load(&root)?;
    for id in ids {
        if !project.config.refs.iter().any(|e| &e.id == id) {
            let known: Vec<&str> = project.config.refs.iter().map(|e| e.id.as_str()).collect();
            let mut err = Error::new(format!("unknown ref '{id}'"))
                .context(format!("refs: {}", if known.is_empty() { "(none)".to_string() } else { known.join(", ") }));
            if let Some(close) = crate::util::closest(id, &known) {
                err = err.hint(format!("did you mean '{close}'?"));
            }
            return Err(err);
        }
    }
    let dir = project.dir_path();
    let st = state::load(&dir)?;
    let settings = settings::load()?;
    let shared = Store::shared_path(settings.store.as_deref()).ok();
    let others = other_projects(shared.as_deref(), &root);

    let refs = project
        .config
        .refs
        .iter()
        .filter(|e| ids.is_empty() || ids.contains(&e.id))
        .map(|entry| details(&project, entry, st.refs.get(&entry.id), shared.as_deref(), &others))
        .collect();
    Ok(ShowReport { project: root.display().to_string(), refs })
}

fn details(
    project: &Project,
    entry: &RefEntry,
    old: Option<&RefState>,
    shared: Option<&Path>,
    others: &[(PathBuf, String, RefState)],
) -> RefDetails {
    let dir = project.dir_path();
    let copy = dir.join(&entry.id);
    let synced = old.is_some() && copy.is_dir();
    let key = store_key(&entry.url);
    let mut d = RefDetails {
        id: entry.id.clone(),
        url: entry.url.clone(),
        git_ref: entry.git_ref.clone(),
        synced,
        kind: None,
        tracks: None,
        commit: None,
        copied_at: None,
        checked_at: None,
        copy_mode: None,
        path: format!("{}/{}", project.config.dir, entry.id),
        files: None,
        bytes: None,
        include: entry.include.clone(),
        exclude: entry.exclude.clone(),
        history: entry.history,
        release_notes: None,
        elsewhere: others
            .iter()
            .filter(|(_, _, s)| store_key(&s.url) == key)
            .map(|(p, id, s)| Elsewhere { project: p.display().to_string(), id: id.clone(), commit: s.commit.clone() })
            .collect(),
        config_changed: false,
    };
    let notes = dir.join(crate::releases::file_name(&entry.id));
    if notes.is_file() {
        d.release_notes = Some(format!("{}/{}", project.config.dir, crate::releases::file_name(&entry.id)));
    }
    let Some(old) = old.filter(|_| synced) else { return d };
    d.kind = Some(old.kind);
    d.tracks = Some(crate::sync::tracks(old.kind, &old.name));
    d.copied_at = Some(old.synced_at);
    d.checked_at = Some(old.checked_at.max(old.synced_at));
    d.copy_mode = Some(old.copy_mode);
    d.config_changed = !old.matches(entry);
    // The commit's description lives wherever its objects are: the history clone, or a store.
    let mut git_dirs = Vec::new();
    if old.copy_mode == CopyMode::Git {
        git_dirs.push(copy.join(".git"));
    }
    if let Some(shared) = shared {
        git_dirs.push(shared.join("repos").join(format!("{key}.git")));
    }
    git_dirs.push(dir.join(FALLBACK_DIR).join("repos").join(format!("{key}.git")));
    d.commit = Some(git_dirs.iter().find_map(|g| describe(g, &old.commit)).unwrap_or(CommitInfo {
        sha: old.commit.clone(),
        date: String::new(),
        author: String::new(),
        subject: String::new(),
    }));
    let (files, bytes) = count(&copy, old.copy_mode == CopyMode::Git);
    d.files = Some(files);
    d.bytes = Some(bytes);
    d
}

pub fn describe(git_dir: &Path, sha: &str) -> Option<CommitInfo> {
    if !git_dir.is_dir() {
        return None;
    }
    let dir = git_dir.to_string_lossy();
    let out = git::run(None, &["--git-dir", &dir, "log", "-1", "--format=%cs%x00%an%x00%s", sha]).ok()?;
    let mut parts = out.trim_end().splitn(3, '\0');
    Some(CommitInfo {
        sha: sha.to_string(),
        date: parts.next()?.to_string(),
        author: parts.next()?.to_string(),
        subject: parts.next().unwrap_or("").to_string(),
    })
}

/// Files and their total size under `dir` (skipping `.git` for history clones).
fn count(dir: &Path, skip_git: bool) -> (u64, u64) {
    let (mut files, mut bytes) = (0, 0);
    let Ok(entries) = std::fs::read_dir(dir) else { return (0, 0) };
    for entry in entries.flatten() {
        if skip_git && entry.file_name() == ".git" {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            let (f, b) = count(&entry.path(), false);
            files += f;
            bytes += b;
        } else {
            files += 1;
            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    (files, bytes)
}

/// Every other registered project's synced refs: (project, id, state).
fn other_projects(shared: Option<&Path>, current: &Path) -> Vec<(PathBuf, String, RefState)> {
    let Some(shared) = shared else { return Vec::new() };
    let store = Store { root: shared.to_path_buf(), shared: true };
    let Ok(registry) = store.read_registry() else { return Vec::new() };
    let mut out = Vec::new();
    for root in registry.projects.keys().map(PathBuf::from) {
        if root == current {
            continue;
        }
        let Ok(project) = Project::load(&root) else { continue };
        let Ok(st) = state::load(&project.dir_path()) else { continue };
        for (id, s) in st.refs {
            out.push((root.clone(), id, s));
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreTree {
    pub commit: CommitInfo,
    pub bytes: u64,
    /// `(project, id)` pairs whose copy is this commit; empty means `gc` removes it.
    pub used_by: Vec<Elsewhere>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreRepo {
    pub key: String,
    pub bytes: u64,
    pub trees: Vec<StoreTree>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreProject {
    pub project: String,
    pub refs: usize,
    pub synced_at: u64,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreReport {
    pub path: String,
    pub bytes: u64,
    pub projects: Vec<StoreProject>,
    pub repos: Vec<StoreRepo>,
}

pub fn store() -> Result<StoreReport> {
    let settings = settings::load()?;
    let root = Store::shared_path(settings.store.as_deref())?;
    let store = Store { root: root.clone(), shared: true };
    let registry = store.read_registry()?;

    // Who uses which (key, commit), from every registered project's state.
    let mut users: BTreeMap<(String, String), Vec<Elsewhere>> = BTreeMap::new();
    let mut projects = Vec::new();
    for (path, entry) in &registry.projects {
        let root = PathBuf::from(path);
        let loaded = Project::load(&root).ok().and_then(|p| state::load(&p.dir_path()).ok());
        projects.push(StoreProject {
            project: path.clone(),
            refs: loaded.as_ref().map(|s| s.refs.len()).unwrap_or(0),
            synced_at: entry.synced_at,
            exists: root.join(config::FILE).is_file(),
        });
        for (id, s) in loaded.map(|s| s.refs).unwrap_or_default() {
            if !s.history {
                users.entry((store_key(&s.url), s.commit.clone())).or_default().push(Elsewhere {
                    project: path.clone(),
                    id,
                    commit: s.commit,
                });
            }
        }
    }

    let mut trees_by_key: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for (key, dir) in crate::gc::sha_dirs(&root.join("trees")) {
        trees_by_key.entry(key).or_default().push(dir);
    }
    let mut repos = Vec::new();
    for (key, repo) in crate::gc::repo_dirs(&root.join("repos")) {
        let mut trees = Vec::new();
        for tree in trees_by_key.remove(&key).unwrap_or_default() {
            let sha = tree.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let commit = describe(&repo, &sha).unwrap_or(CommitInfo {
                sha: sha.clone(),
                date: String::new(),
                author: String::new(),
                subject: String::new(),
            });
            let used_by = users.get(&(key.clone(), sha)).cloned().unwrap_or_default();
            trees.push(StoreTree { commit, bytes: crate::gc::size(&tree), used_by });
        }
        trees.sort_by(|a, b| b.commit.date.cmp(&a.commit.date));
        let bytes = crate::gc::size(&repo) + trees.iter().map(|t| t.bytes).sum::<u64>();
        repos.push(StoreRepo { key, bytes, trees });
    }
    Ok(StoreReport { path: root.display().to_string(), bytes: crate::gc::size(&root), projects, repos })
}
