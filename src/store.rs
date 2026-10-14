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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreMode {
    Shared,
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreLocation {
    pub path: String,
    pub mode: StoreMode,
}

pub struct Store {
    pub root: PathBuf,
    /// False for the fallback store inside a refs folder.
    pub shared: bool,
}

impl Store {
    pub fn location(&self) -> StoreLocation {
        StoreLocation { path: self.root.display().to_string(), mode: if self.shared { StoreMode::Shared } else { StoreMode::Fallback } }
    }

    /// Import objects from a local fallback repo and publish a complete tree.
    /// Cached trees are complete by construction; never use a filtered project copy.
    pub fn import(&self, source: &Store, key: &str, sha: &str) -> std::result::Result<bool, String> {
        let _lock = self.lock_repo(key).map_err(|e| e.to_string())?;
        let repo = self.ensure_repo(key)?;
        let tree = self.tree_path(key, sha);
        let complete = tree.is_dir() && git::has_commit(&repo, sha);
        if complete {
            return Ok(false);
        }
        if !git::has_commit(&repo, sha) {
            let source_repo = source.repo_path(key).canonicalize().map_err(|e| e.to_string())?;
            let url = format!("file://{}", source_repo.display());
            git::run(Some(&repo), &["fetch", "--quiet", "--depth", "1", "--no-tags", "--no-write-fetch-head", &url, sha])
                .map_err(|e| e.to_string())?;
        }
        if !git::has_commit(&repo, sha) {
            return Err(format!("imported repo does not hold commit {sha}"));
        }
        self.keep(&repo, sha)?;
        if !tree.is_dir() {
            let cached = source.tree_path(key, sha);
            if cached.is_dir() {
                let tmp = self.root.join("tmp").join(unique_name("promoted-tree"));
                let result = (|| {
                    copy::clone_tree(&cached, &tmp).map_err(|e| e.to_string())?;
                    copy::make_read_only(&tmp, None).map_err(|e| e.to_string())?;
                    if let Some(parent) = tree.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::rename(&tmp, &tree).map_err(|e| e.to_string())
                })();
                if result.is_err() {
                    let _ = copy::remove(&tmp);
                }
                result?;
            } else {
                self.extract(&repo, sha, &tree)?;
            }
        }
        Ok(true)
    }
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

    /// `fold_case`, only when no other tiger process is using the store.
    pub fn fold_case_when_alone(&self) {
        if let Ok(file) = self.lock_file("store.lock")
            && file.try_lock().is_ok()
        {
            self.fold_case();
        }
    }

    /// Renames folders stored before tiger folded letter case on hosts that
    /// ignore it (`repos/github.com/TanStack/router.git` becomes
    /// `.../tanstack/router.git`). Lookups already ignore case there; this
    /// keeps the folder names tidy. Only owner and repo folders are renamed,
    /// never anything inside a repo or tree. When a lowercase folder already
    /// exists, the older one is left for `gc`. The caller holds the store alone.
    pub fn fold_case(&self) {
        for host in crate::url::CASE_INSENSITIVE_HOSTS {
            fold_names(&self.root.join("repos").join(host), &|name| name.ends_with(".git"));
            fold_names(&self.root.join("trees").join(host), &|name| crate::git::is_full_sha(name));
        }
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
    pub fn lock_shared(&self, on_wait: &dyn Fn()) -> Result<File> {
        let file = self.lock_file("store.lock")?;
        crate::util::lock_or_wait(&file, true, on_wait).at("lock", &self.root)?;
        Ok(file)
    }

    /// Held by `gc`: no sync runs meanwhile.
    pub fn lock_exclusive(&self, on_wait: &dyn Fn()) -> Result<File> {
        let file = self.lock_file("store.lock")?;
        crate::util::lock_or_wait(&file, false, on_wait).at("lock", &self.root)?;
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
        // A tag's own ID peels to its commit; trees are always stored by commit.
        let commit = match git::peel(&repo, sha) {
            Some(commit) => commit,
            None => self.fetch(&repo, url, fetch_ref, sha)?,
        };
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

/// Lowercases folder names under `dir`, stopping at leaves (a bare repo or a
/// commit's tree), whose contents keep their names.
fn fold_names(dir: &Path, is_leaf: &dyn Fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let mut path = entry.path();
        let lower = name.to_ascii_lowercase();
        if lower != name {
            let target = dir.join(&lower);
            // On a case-insensitive disk `target` *is* `path`, so rename through a temporary name.
            let same = target.canonicalize().ok() == path.canonicalize().ok();
            if same || !target.exists() {
                let staging = dir.join(crate::util::unique_name(".tiger-fold"));
                if std::fs::rename(&path, &staging).is_ok() {
                    if std::fs::rename(&staging, &target).is_ok() {
                        path = target;
                    } else {
                        let _ = std::fs::rename(&staging, &path);
                    }
                }
            }
        }
        if !is_leaf(&name) {
            fold_names(&path, is_leaf);
        }
    }
}
