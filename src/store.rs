//! The store: one shallow bare clone per repository and one read-only tree
//! per commit in use, shared by every project on the machine. Deleting it
//! loses nothing; every project rebuilds from its config.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::copy;
use crate::error::{Error, IoContext, Result};
use crate::git;
use crate::util::{unique_name, write_atomic};

/// The store's location when the shared one isn't writable: inside the refs folder.
pub const FALLBACK_DIR: &str = ".tiger-store";

pub struct Store {
    pub root: PathBuf,
    /// False for the fallback store inside a refs folder.
    pub shared: bool,
}

impl Store {
    /// The shared store's path: `$TIGER_HOME`, the `store` setting, or `~/.tiger`.
    pub fn shared_path(setting: Option<&str>) -> Result<PathBuf> {
        if let Some(home) = std::env::var_os("TIGER_HOME").filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(home));
        }
        if let Some(path) = setting {
            return Ok(expand_home(path));
        }
        crate::util::home_dir()
            .map(|h| h.join(".tiger"))
            .ok_or_else(|| Error::new("could not find the home directory").hint("set TIGER_HOME to the store's path"))
    }

    /// Opens the shared store, or `None` when it can't be written.
    pub fn open_shared(setting: Option<&str>) -> Result<Option<Store>> {
        let root = Store::shared_path(setting)?;
        let store = Store { root, shared: true };
        Ok(if store.is_writable() { Some(store) } else { None })
    }

    pub fn open_fallback(refs_dir: &Path) -> Result<Store> {
        let store = Store { root: refs_dir.join(FALLBACK_DIR), shared: false };
        if !store.is_writable() {
            return Err(
                Error::new(format!("could not write the store at {}", store.root.display())).hint("check the refs folder's permissions")
            );
        }
        Ok(store)
    }

    fn is_writable(&self) -> bool {
        let tmp = self.root.join("tmp");
        if std::fs::create_dir_all(&tmp).is_err() {
            return false;
        }
        let probe = tmp.join(unique_name("probe"));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    pub fn repo_path(&self, key: &str) -> PathBuf {
        self.root.join("repos").join(format!("{key}.git"))
    }

    pub fn tree_path(&self, key: &str, sha: &str) -> PathBuf {
        self.root.join("trees").join(key).join(sha)
    }

    fn lock_file(&self, name: &str) -> Result<File> {
        let dir = self.root.join("locks");
        std::fs::create_dir_all(&dir).at("create", &dir)?;
        let path = dir.join(name);
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).at("open", &path)
    }

    /// Held while syncing: many syncs at once, but never during `gc`.
    pub fn lock_shared(&self) -> Result<File> {
        let file = self.lock_file("store.lock")?;
        file.lock_shared().at("lock", &self.root)?;
        Ok(file)
    }

    /// Held by `gc`: no sync runs meanwhile.
    pub fn lock_exclusive(&self) -> Result<File> {
        let file = self.lock_file("store.lock")?;
        file.lock().at("lock", &self.root)?;
        Ok(file)
    }

    fn lock_repo(&self, key: &str) -> Result<File> {
        let file = self.lock_file(&format!("repo-{}.lock", crate::util::digest(key.as_bytes())))?;
        file.lock().at("lock", &self.root)?;
        Ok(file)
    }

    /// Makes sure the store holds the tree of `sha`, fetching it if needed,
    /// and returns the commit actually extracted (a branch may have moved
    /// since `ls-remote`) with the tree's path.
    pub fn ensure_tree(&self, url: &str, key: &str, fetch_ref: Option<&str>, sha: &str) -> std::result::Result<(String, PathBuf), String> {
        let tree = self.tree_path(key, sha);
        if tree.is_dir() {
            return Ok((sha.to_string(), tree));
        }
        let _lock = self.lock_repo(key).map_err(|e| e.message)?;
        if tree.is_dir() {
            return Ok((sha.to_string(), tree));
        }
        let repo = self.ensure_repo(key)?;
        let commit = if git::has_commit(&repo, sha) { sha.to_string() } else { self.fetch(&repo, url, fetch_ref, sha)? };
        let tree = self.tree_path(key, &commit);
        if !tree.is_dir() {
            self.extract(&repo, &commit, &tree)?;
        }
        Ok((commit, tree))
    }

    fn ensure_repo(&self, key: &str) -> std::result::Result<PathBuf, String> {
        let repo = self.repo_path(key);
        if repo.join("HEAD").is_file() {
            return Ok(repo);
        }
        let tmp = self.root.join("tmp").join(unique_name("repo"));
        let tmp_str = tmp.to_string_lossy();
        git::run(None, &["init", "--bare", "--quiet", &tmp_str]).map_err(|e| e.to_string())?;
        // Keep the store's objects: they back trees that projects still use.
        git::run(Some(&tmp), &["config", "gc.auto", "0"]).map_err(|e| e.to_string())?;
        if let Some(parent) = repo.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &repo).map_err(|e| format!("could not create {}: {e}", repo.display()))?;
        Ok(repo)
    }

    fn fetch(&self, repo: &Path, url: &str, fetch_ref: Option<&str>, sha: &str) -> std::result::Result<String, String> {
        match fetch_ref {
            Some(full_ref) => {
                let refspec = format!("+{full_ref}:refs/tiger/fetch");
                git::run(Some(repo), &["fetch", "--quiet", "--depth", "1", "--no-tags", url, &refspec]).map_err(|e| e.to_string())?;
                let commit =
                    git::run(Some(repo), &["rev-parse", "refs/tiger/fetch^{commit}"]).map_err(|e| e.to_string())?.trim().to_string();
                self.keep(repo, &commit)?;
                Ok(commit)
            }
            None => {
                if git::run(Some(repo), &["fetch", "--quiet", "--depth", "1", "--no-tags", url, sha]).is_err() {
                    // Some servers refuse fetching a commit by SHA; fetch every branch and tag instead.
                    git::run(
                        Some(repo),
                        &[
                            "fetch",
                            "--quiet",
                            "--no-tags",
                            url,
                            "+refs/heads/*:refs/tiger/all/heads/*",
                            "+refs/tags/*:refs/tiger/all/tags/*",
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                }
                if !git::has_commit(repo, sha) {
                    return Err(format!("commit {sha} is not on the remote"));
                }
                self.keep(repo, sha)?;
                Ok(sha.to_string())
            }
        }
    }

    fn keep(&self, repo: &Path, sha: &str) -> std::result::Result<(), String> {
        git::run(Some(repo), &["update-ref", &format!("refs/tiger/keep/{sha}"), sha]).map(|_| ()).map_err(|e| e.to_string())
    }

    fn extract(&self, repo: &Path, sha: &str, tree: &Path) -> std::result::Result<(), String> {
        let tmp = self.root.join("tmp").join(unique_name("tree"));
        let index = self.root.join("tmp").join(unique_name("index"));
        let result = (|| {
            std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            for args in [vec!["read-tree", sha], vec!["checkout-index", "--all", "--force"]] {
                let mut cmd = git::command();
                cmd.env("GIT_INDEX_FILE", &index).arg("--git-dir").arg(repo).arg("--work-tree").arg(&tmp).args([
                    "-c",
                    "core.autocrlf=false",
                    "-c",
                    "core.symlinks=true",
                ]);
                git::run_with(cmd, None, &args).map_err(|e| e.to_string())?;
            }
            copy::make_read_only(&tmp, None).map_err(|e| e.to_string())?;
            if let Some(parent) = tree.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&tmp, tree).map_err(|e| format!("could not store tree {}: {e}", tree.display()))
        })();
        let _ = std::fs::remove_file(&index);
        if result.is_err() {
            let _ = copy::remove(&tmp);
        }
        result
    }

    pub fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }

    /// Reads, changes, and writes the registry under its lock.
    pub fn update_registry<T>(&self, change: impl FnOnce(&mut Registry) -> T) -> Result<T> {
        let lock = self.lock_file("registry.lock")?;
        lock.lock().at("lock", &self.root)?;
        let mut registry = self.read_registry()?;
        let out = change(&mut registry);
        let mut text = serde_json::to_string_pretty(&registry).expect("registry serializes");
        text.push('\n');
        write_atomic(&self.registry_path(), text.as_bytes())?;
        Ok(out)
    }

    pub fn read_registry(&self) -> Result<Registry> {
        let path = self.registry_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|err| {
                Error::new(format!("could not read {}", path.display()))
                    .context(format!("cause: {err}"))
                    .hint("delete it; each project registers again on its next sync")
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(err) => Err(Error::io("read", &path, err)),
        }
    }
}

/// The projects that use this store, so `--everywhere` and `gc` can find them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    /// Project root → when it last synced.
    pub projects: BTreeMap<String, RegistryEntry>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry { version: 1, projects: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub synced_at: u64,
}

pub fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), crate::util::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => crate::util::home_dir().unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}
