//! The commands that edit `tiger.toml`: add, rm, pin, unpin. Each checks
//! everything before writing, so a failed command leaves the config as it was.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::config::{self, Project, validate_id};
use crate::copy;
use crate::edit::{self, NewRef};
use crate::error::{Error, Result};
use crate::init::{self, Initialized};
use crate::remote::{self, Kind, Want};
use crate::settings;
use crate::state;
use crate::sync::{self, Outcome};
use crate::url;

#[derive(Debug, Clone, Default)]
pub struct AddArgs {
    pub url: String,
    pub id: Option<String>,
    pub git_ref: Option<String>,
    pub note: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub history: bool,
    pub releases: bool,
    pub no_propagate: bool,
    pub no_sync: bool,
}

impl AddArgs {
    fn has_options(&self) -> bool {
        self.git_ref.is_some() || !self.include.is_empty() || !self.exclude.is_empty() || self.history || self.releases || self.no_propagate
    }
}

#[derive(Debug, Serialize)]
pub struct Added {
    pub id: String,
    pub url: String,
    /// False when the config already had this ref.
    pub added: bool,
    /// What the ref follows or pins, as resolved when it was added.
    pub tracks: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialized: Option<Initialized>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<Outcome>,
}

pub fn add(start: &Path, args: &AddArgs) -> Result<Added> {
    let mut url = url::expand(&args.url);
    if !url.contains("://") && !url.contains(':') && Path::new(&url).is_dir() {
        // A local path: store it absolute, so it means the same from anywhere.
        url = Path::new(&url).canonicalize().map(|p| p.display().to_string()).unwrap_or(url);
    }
    let settings = settings::load()?;
    let (root, initialized) = match config::find_root(start) {
        Some(root) => (root, None),
        None => {
            let root = init::default_root(start);
            let done = init::init(&root, settings.dir(), true)?;
            (root, Some(done))
        }
    };
    let project = Project::load(&root)?;

    let id = args.id.clone().unwrap_or_else(|| url::default_id(&url));
    validate_id(&id).map_err(|reason| Error::new(format!("can't use id {id:?}")).context(reason).hint("pass --as <id>"))?;

    let same_id = project.config.refs.iter().find(|e| e.id == id);
    let same_url = project.config.refs.iter().find(|e| url::store_key(&e.url) == url::store_key(&url) && args.id.is_none());
    if let Some(existing) = same_id.or(same_url) {
        if url::store_key(&existing.url) != url::store_key(&url) {
            return Err(Error::new(format!("ref '{id}' already exists for another repo"))
                .context(format!("refs.{id}: {}", existing.url))
                .hint("pass --as <id> to choose another id"));
        }
        if args.has_options() {
            return Err(Error::new(format!("'{}' is already a ref", existing.id))
                .hint(format!("change its options in tiger.toml, or use `tiger pin {}`", existing.id)));
        }
        let outcome = if args.no_sync {
            None
        } else {
            Some(sync::sync(&root, &sync::Options { ids: vec![existing.id.clone()], ..Default::default() })?)
        };
        return Ok(Added { id: existing.id.clone(), url: existing.url.clone(), added: false, tracks: None, initialized, sync: outcome });
    }

    // Reachable, and the ref exists, before anything is written.
    let want = Want::parse(args.git_ref.as_deref());
    let tracks = if let Want::Commit(sha) = &want {
        crate::git::run(None, &["ls-remote", &url, "HEAD"]).map_err(|e| unreachable(&url, &e.to_string()))?;
        format!("pinned to commit {}", crate::util::short_sha(sha))
    } else {
        let patterns: BTreeSet<String> = want.patterns().into_iter().collect();
        let remote = remote::ls_remote(&url, &patterns).map_err(|e| unreachable(&url, &e.to_string()))?;
        let resolved = remote::resolve(&want, &remote).map_err(|reason| Error::new(format!("can't add {url}")).context(reason))?;
        match resolved.kind {
            Kind::Branch if resolved.is_default => format!("follows {}, the default branch", resolved.name),
            Kind::Branch => format!("follows {}", resolved.name),
            Kind::Tag => format!("pinned to tag {}", resolved.name),
            Kind::Commit => format!("pinned to commit {}", crate::util::short_sha(&resolved.name)),
        }
    };

    let path = project.config_path();
    let mut doc = edit::load(&path)?;
    let new = NewRef {
        id: id.clone(),
        url: url.clone(),
        git_ref: args.git_ref.clone(),
        include: args.include.clone(),
        exclude: args.exclude.clone(),
        history: args.history,
        releases: args.releases,
        propagate: args.no_propagate.then_some(false),
        note: args.note.clone(),
    };
    edit::add(&mut doc, &new)?;
    edit::save(&path, &doc)?;

    let outcome =
        if args.no_sync { None } else { Some(sync::sync(&root, &sync::Options { ids: vec![id.clone()], ..Default::default() })?) };
    Ok(Added { id, url, added: true, tracks: Some(tracks), initialized, sync: outcome })
}

fn unreachable(url: &str, reason: &str) -> Error {
    Error::new(format!("can't reach {url}"))
        .context(reason.to_string())
        .hint("check the URL; private repos need SSH keys or a git credential helper (tiger never prompts)")
}

#[derive(Debug, Serialize)]
pub struct Removed {
    pub id: String,
    /// The folder tiger deleted, if it had created one.
    pub deleted: Option<String>,
}

pub fn rm(start: &Path, ids: &[String]) -> Result<Vec<Removed>> {
    let project = load(start)?;
    for id in ids {
        known(&project, id)?;
    }
    let path = project.config_path();
    let mut doc = edit::load(&path)?;
    for id in ids {
        edit::remove(&mut doc, id)?;
    }
    edit::save(&path, &doc)?;

    let dir = project.dir_path();
    let _lock = state::lock(&dir)?;
    let mut st = state::load(&dir)?;
    let mut removed = Vec::new();
    for id in ids {
        let mut deleted = None;
        if st.refs.remove(id).is_some() {
            let folder = dir.join(id);
            copy::remove(&folder).map_err(|e| Error::io("remove", &folder, e))?;
            let _ = copy::remove(&dir.join(crate::releases::file_name(id)));
            deleted = Some(format!("{}/{id}", project.config.dir));
        }
        removed.push(Removed { id: id.clone(), deleted });
    }
    state::save(&dir, &st)?;
    Ok(removed)
}

#[derive(Debug, Serialize)]
pub struct Pinned {
    pub id: String,
    /// The new `ref` value; `None` after unpin.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<Outcome>,
}

pub fn pin(start: &Path, id: &str, target: Option<&str>, no_sync: bool) -> Result<Pinned> {
    let project = load(start)?;
    let entry = known(&project, id)?.clone();
    let git_ref = match target {
        None => {
            let st = state::load(&project.dir_path())?;
            let old = st.refs.get(id).ok_or_else(|| {
                Error::new(format!("'{id}' hasn't been synced, so there's no commit to pin"))
                    .hint(format!("run `tiger sync {id}` first, or pass a tag or commit: `tiger pin {id} <tag|sha>`"))
            })?;
            old.commit.clone()
        }
        Some(target) => match Want::parse(Some(target)) {
            Want::Commit(sha) => sha,
            want => {
                let patterns: BTreeSet<String> = want.patterns().into_iter().collect();
                let remote = remote::ls_remote(&entry.url, &patterns).map_err(|e| unreachable(&entry.url, &e.to_string()))?;
                let resolved = remote::resolve(&want, &remote)
                    .map_err(|reason| Error::new(format!("can't pin '{id}' to {target}")).context(reason))?;
                if resolved.kind == Kind::Branch {
                    return Err(Error::new(format!("'{target}' is a branch, and a pin is a tag or commit"))
                        .hint(format!("to follow that branch, set ref = \"{target}\" for {id} in tiger.toml; to pin its current commit, run `tiger pin {id} {}`", resolved.commit)));
                }
                target.to_string()
            }
        },
    };
    set_ref(&project, id, Some(&git_ref))?;
    let sync =
        if no_sync { None } else { Some(sync::sync(&project.root, &sync::Options { ids: vec![id.to_string()], ..Default::default() })?) };
    Ok(Pinned { id: id.to_string(), git_ref: Some(git_ref), sync })
}

pub fn unpin(start: &Path, id: &str, no_sync: bool) -> Result<Pinned> {
    let project = load(start)?;
    known(&project, id)?;
    set_ref(&project, id, None)?;
    let sync =
        if no_sync { None } else { Some(sync::sync(&project.root, &sync::Options { ids: vec![id.to_string()], ..Default::default() })?) };
    Ok(Pinned { id: id.to_string(), git_ref: None, sync })
}

fn set_ref(project: &Project, id: &str, git_ref: Option<&str>) -> Result<()> {
    let path = project.config_path();
    let mut doc = edit::load(&path)?;
    edit::set_ref(&mut doc, id, git_ref)?;
    edit::save(&path, &doc)
}

fn load(start: &Path) -> Result<Project> {
    let root = config::find_root(start).ok_or_else(|| {
        Error::new(format!("no {} in {} or any parent folder", config::FILE, start.display())).hint("run `tiger init` in the project root")
    })?;
    Project::load(&root)
}

fn known<'a>(project: &'a Project, id: &str) -> Result<&'a config::RefEntry> {
    project.config.refs.iter().find(|e| e.id == id).ok_or_else(|| {
        let ids: Vec<&str> = project.config.refs.iter().map(|e| e.id.as_str()).collect();
        let mut err = Error::new(format!("unknown ref '{id}'"))
            .context(format!("refs: {}", if ids.is_empty() { "(none)".into() } else { ids.join(", ") }));
        if let Some(close) = crate::util::closest(id, &ids) {
            err = err.hint(format!("did you mean '{close}'?"));
        }
        err
    })
}
