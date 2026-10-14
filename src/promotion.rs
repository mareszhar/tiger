//! Move disposable cache data from a project's fallback to the shared store.
//! The caller holds both stores' locks and the project's lock.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::config::RefEntry;
use crate::state::State;
use crate::store::Store;
use crate::url::store_key;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub promoted_trees: usize,
    pub remaining_trees: usize,
    pub remaining_repos: usize,
    /// Copies re-cloned from the shared store so they share its disk space.
    pub relinked: usize,
    pub failures: Vec<Failure>,
    pub cleanup_warnings: Vec<String>,
    #[serde(skip)]
    imported: BTreeSet<(String, String)>,
    /// Commits the shared store already had its own tree for: copies made
    /// from the fallback store hold separate disk blocks until re-cloned.
    #[serde(skip)]
    already_shared: BTreeSet<(String, String)>,
}

impl Report {
    pub fn needs_relink(&self, key: &str, sha: &str) -> bool {
        self.already_shared.contains(&(key.to_string(), sha.to_string()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub repo: String,
    pub commit: String,
    pub error: String,
}

pub fn import(source: &Store, destination: &Store, state: &State, entries: &[RefEntry], activity: &crate::progress::Activity) -> Report {
    let selected: BTreeSet<_> = entries.iter().map(|e| e.id.as_str()).collect();
    let wanted: BTreeSet<_> = state
        .refs
        .iter()
        .filter(|(id, r)| selected.contains(id.as_str()) && !r.history)
        .map(|(_, r)| (store_key(&r.url), r.commit.clone()))
        .collect();
    let mut report = Report::default();
    activity.quietly("moving fallback data to the shared store", wanted.len());
    for (key, sha) in wanted {
        let cached = cache_exists(&source.tree_path(&key, &sha))
            .and_then(|tree| cache_exists(&source.repo_path(&key).join("HEAD")).map(|repo| tree || repo));
        let result = match cached {
            Ok(true) => destination.import(source, &key, &sha).map(Some),
            Ok(false) => Ok(None),
            Err(error) => Err(format!("could not inspect fallback data: {error}")),
        };
        match result {
            Ok(Some(created)) => {
                report.promoted_trees += usize::from(created);
                if !created {
                    report.already_shared.insert((key.clone(), sha.clone()));
                }
                report.imported.insert((key, sha));
            }
            Err(error) => report.failures.push(Failure { repo: key, commit: sha, error }),
            Ok(None) => {}
        }
        activity.done();
    }
    observe(source, &mut report);
    report
}

pub fn cleanup(source: &Store, state: &State, entries: &[RefEntry], full: bool, report: &mut Report) {
    match source.read_registry() {
        Ok(registry) if registry.projects.is_empty() => {}
        result => {
            report.cleanup_warnings.push(match result {
                Ok(_) => "fallback store has registered projects; retained its cache data".to_string(),
                Err(error) => format!("could not read fallback registry; retained cache data: {error}"),
            });
            observe(source, report);
            return;
        }
    }
    let selected: BTreeSet<_> = entries.iter().map(|e| e.id.as_str()).collect();
    let retained: BTreeSet<_> = state
        .refs
        .iter()
        .filter(|(id, r)| !selected.contains(id.as_str()) && !r.history)
        .map(|(_, r)| (store_key(&r.url), r.commit.clone()))
        .collect();
    let full_cleanup = full && report.failures.is_empty();
    for (key, path) in crate::gc::sha_dirs(&source.root.join("trees")) {
        let sha = path.file_name().unwrap().to_string_lossy().to_string();
        let item = (key, sha);
        if !retained.contains(&item)
            && (full_cleanup || report.imported.contains(&item))
            && let Err(error) = crate::copy::remove(&path)
        {
            report.cleanup_warnings.push(format!("could not remove redundant fallback tree {}: {error}", path.display()));
        }
    }
    let remaining: BTreeSet<_> = crate::gc::sha_dirs(&source.root.join("trees")).into_iter().map(|(key, _)| key).collect();
    for (key, repo) in crate::gc::repo_dirs(&source.root.join("repos")) {
        let selected_repo = report.imported.iter().any(|(imported, _)| imported == &key);
        if (full_cleanup || selected_repo)
            && !remaining.contains(&key)
            && !retained.iter().any(|(kept, _)| kept == &key)
            && let Err(error) = crate::copy::remove(&repo)
        {
            report.cleanup_warnings.push(format!("could not remove redundant fallback repo {}: {error}", repo.display()));
        }
    }
    // Keep the source root and locks. A waiting sync may already hold a lock fd.
    // Unknown files and temporary work are not blanket-deleted.
    observe(source, report);
}

fn observe(source: &Store, report: &mut Report) {
    report.remaining_trees = crate::gc::sha_dirs(&source.root.join("trees")).len();
    report.remaining_repos = crate::gc::repo_dirs(&source.root.join("repos")).len();
}

fn cache_exists(path: &std::path::Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
