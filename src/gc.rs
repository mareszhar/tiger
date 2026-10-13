//! `tiger gc`: prune what no project uses from the shared store.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{self, Project};
use crate::copy;
use crate::error::{Error, Result};
use crate::git;
use crate::settings;
use crate::state;
use crate::store::Store;
use crate::url::store_key;

#[derive(Debug, Default, Serialize)]
pub struct Collected {
    pub store: String,
    pub projects: usize,
    pub forgotten_projects: Vec<String>,
    pub removed_trees: usize,
    pub removed_repos: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

pub fn gc() -> Result<Collected> {
    let settings = settings::load()?;
    let Some(store) = Store::open_shared(settings.store.as_deref())? else {
        let path = Store::shared_path(settings.store.as_deref())?;
        return Err(Error::new(format!("can't write the shared store at {}", path.display())).hint("run `tiger gc` outside the sandbox"));
    };
    let _lock = store.lock_exclusive()?;
    let mut out = Collected { store: store.root.display().to_string(), bytes_before: size(&store.root), ..Collected::default() };

    // Commits in use, per store key, from every registered project's state.
    let registry = store.read_registry()?;
    let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut forgotten = Vec::new();
    for root in registry.projects.keys().map(PathBuf::from) {
        if !root.join(config::FILE).is_file() {
            forgotten.push(root.display().to_string());
            continue;
        }
        // A project gc can't read might use anything; stop rather than guess.
        let project = Project::load(&root).map_err(|e| {
            e.context("tiger gc stopped without deleting anything").hint(format!("fix {}/{} first", root.display(), config::FILE))
        })?;
        out.projects += 1;
        let st = state::load(&project.dir_path())?;
        for r in st.refs.values().filter(|r| !r.history) {
            used.entry(store_key(&r.url)).or_default().insert(r.commit.clone());
        }
    }
    if !forgotten.is_empty() {
        store.update_registry(|registry| {
            for key in &forgotten {
                registry.projects.remove(key);
            }
        })?;
    }
    out.forgotten_projects = forgotten;

    // Trees: trees/<key>/<sha>. Keys are host/owner/repo of any depth, so find sha folders by name.
    let trees = store.root.join("trees");
    for (key, sha_dir) in sha_dirs(&trees) {
        if !used.get(&key).is_some_and(|shas| sha_dir.file_name().is_some_and(|n| shas.contains(&n.to_string_lossy().to_string()))) {
            copy::remove(&sha_dir).map_err(|e| Error::io("remove", &sha_dir, e))?;
            out.removed_trees += 1;
        }
    }
    remove_empty_dirs(&trees);

    // Repos: drop unused ones; in the rest, drop keep-refs of unused commits and prune.
    let repos = store.root.join("repos");
    for (key, repo) in repo_dirs(&repos) {
        match used.get(&key) {
            None => {
                copy::remove(&repo).map_err(|e| Error::io("remove", &repo, e))?;
                out.removed_repos += 1;
            }
            Some(shas) => {
                let refs = git::run(Some(&repo), &["for-each-ref", "--format=%(refname)", "refs/tiger/"]).unwrap_or_default();
                for refname in refs.lines() {
                    let keep = refname.strip_prefix("refs/tiger/keep/").is_some_and(|sha| shas.contains(sha));
                    if !keep {
                        let _ = git::run(Some(&repo), &["update-ref", "-d", refname]);
                    }
                }
                let _ = git::run(Some(&repo), &["reflog", "expire", "--expire=now", "--all"]);
                let _ = git::run(Some(&repo), &["gc", "--quiet", "--prune=now"]);
            }
        }
    }
    remove_empty_dirs(&repos);
    let _ = copy::remove(&store.root.join("tmp"));
    out.bytes_after = size(&store.root);
    Ok(out)
}

fn is_sha(name: &str) -> bool {
    git::is_full_sha(name)
}

pub fn sha_dirs(trees: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    walk_dirs(trees, &mut |dir| {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if is_sha(&name) {
            let key = dir.parent().and_then(|p| p.strip_prefix(trees).ok()).map(|p| p.display().to_string()).unwrap_or_default();
            found.push((key, dir.to_path_buf()));
            return false;
        }
        true
    });
    found
}

pub fn repo_dirs(repos: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    walk_dirs(repos, &mut |dir| {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(stem) = name.strip_suffix(".git") {
            let parent = dir.parent().and_then(|p| p.strip_prefix(repos).ok()).map(|p| p.display().to_string()).unwrap_or_default();
            let key = if parent.is_empty() { stem.to_string() } else { format!("{parent}/{stem}") };
            found.push((key, dir.to_path_buf()));
            return false;
        }
        true
    });
    found
}

/// Visits directories under `root`; `visit` returns whether to descend.
fn walk_dirs(root: &Path, visit: &mut dyn FnMut(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let path = entry.path();
            if visit(&path) {
                walk_dirs(&path, visit);
            }
        }
    }
}

fn remove_empty_dirs(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut empty = true;
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if remove_empty_dirs(&entry.path()) {
                let _ = std::fs::remove_dir(entry.path());
            } else {
                empty = false;
            }
        } else {
            empty = false;
        }
    }
    empty
}

/// Disk usage of a tree, counting each file's allocated blocks.
pub fn size(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.blocks() * 512;
    }
    let mut total = meta.blocks() * 512;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total += size(&entry.path());
        }
    }
    total
}
