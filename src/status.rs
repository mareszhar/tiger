//! `tiger status`: for each ref, what it tracks, the commit on disk, the
//! commit upstream, and whether a sync would change it. Never writes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::config::{self, Project};
use crate::copy::CopyMode;
use crate::error::{Error, Result};
use crate::remote::{self, Kind, Want};
use crate::settings;
use crate::state;
use crate::util::parallel_map;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    UpToDate,
    Behind,
    Pinned,
    NotSynced,
    ConfigChanged,
    Removed,
    Unknown,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::UpToDate => "up to date",
            Verdict::Behind => "behind",
            Verdict::Pinned => "pinned",
            Verdict::NotSynced => "not synced",
            Verdict::ConfigChanged => "config changed",
            Verdict::Removed => "removed from config",
            Verdict::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RefStatus {
    pub id: String,
    pub tracks: String,
    pub local: Option<String>,
    pub upstream: Option<String>,
    pub verdict: Verdict,
    pub copy_mode: Option<CopyMode>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub project: String,
    pub refs: Vec<RefStatus>,
}

pub fn status(start: &Path, offline: bool) -> Result<Report> {
    let root = config::find_root(start).ok_or_else(|| {
        Error::new(format!("no {} in {} or any parent folder", config::FILE, start.display())).hint("run `tiger init` in the project root")
    })?;
    let project = Project::load(&root)?;
    let dir = project.dir_path();
    let state = state::load(&dir)?;
    let concurrency = settings::load()?.concurrency();

    let mut patterns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if !offline {
        for entry in &project.config.refs {
            let want = Want::parse(entry.git_ref.as_deref());
            if !matches!(want, Want::Commit(_)) {
                patterns.entry(entry.url.clone()).or_default().extend(want.patterns());
            }
        }
    }
    let urls: Vec<_> = patterns.into_iter().collect();
    let answers = parallel_map(&urls, concurrency, |(url, p)| remote::ls_remote(url, p));
    let answers: BTreeMap<&str, _> = urls.iter().map(|(u, _)| u.as_str()).zip(answers).collect();

    let mut refs = Vec::new();
    for entry in &project.config.refs {
        let want = Want::parse(entry.git_ref.as_deref());
        let old = state.refs.get(&entry.id);
        let folder = dir.join(&entry.id);
        let synced = old.is_some() && folder.is_dir();
        let upstream = answers
            .get(entry.url.as_str())
            .map(|answer| answer.as_ref().map_err(|e| e.to_string()).and_then(|r| remote::resolve(&want, r)));
        let tracks = match (&want, old) {
            (Want::DefaultBranch, Some(o)) => format!("{} (default branch)", o.name),
            (Want::DefaultBranch, None) => match &upstream {
                Some(Ok(r)) => format!("{} (default branch)", r.name),
                _ => "default branch".into(),
            },
            (Want::Branch(b), _) => b.clone(),
            (Want::Tag(t), _) => format!("tag {t}"),
            (Want::Name(n), Some(o)) if o.kind == Kind::Tag => format!("tag {n}"),
            (Want::Name(n), _) => n.clone(),
            (Want::Commit(c), _) => format!("commit {}", crate::util::short_sha(c)),
        };
        let mut status = RefStatus {
            id: entry.id.clone(),
            tracks,
            local: old.filter(|_| synced).map(|o| o.commit.clone()),
            upstream: None,
            verdict: Verdict::Unknown,
            copy_mode: old.filter(|_| synced).map(|o| o.copy_mode),
            path: format!("{}/{}", project.config.dir, entry.id),
            detail: None,
        };
        match &upstream {
            Some(Ok(r)) => status.upstream = Some(r.commit.clone()),
            Some(Err(err)) => status.detail = Some(err.clone()),
            None => {}
        }
        if let Want::Commit(sha) = &want {
            status.upstream = Some(sha.clone());
        }
        status.verdict = match old {
            _ if !synced => Verdict::NotSynced,
            Some(o) if !o.matches(entry) => Verdict::ConfigChanged,
            Some(o) => match (&want, &upstream) {
                (Want::Commit(_), _) => Verdict::Pinned,
                (_, Some(Ok(r))) if r.commit == o.commit => {
                    if o.kind.is_pin() {
                        Verdict::Pinned
                    } else {
                        Verdict::UpToDate
                    }
                }
                (_, Some(Ok(_))) => {
                    if o.kind == Kind::Tag {
                        status.detail = Some("the tag moved upstream".into());
                    }
                    Verdict::Behind
                }
                (_, None) if o.kind.is_pin() => Verdict::Pinned,
                (_, None) => {
                    status.detail = Some("offline".into());
                    Verdict::Unknown
                }
                (_, Some(Err(_))) => Verdict::Unknown,
            },
            None => Verdict::NotSynced,
        };
        refs.push(status);
    }
    let ids: BTreeSet<&str> = project.config.refs.iter().map(|e| e.id.as_str()).collect();
    for (id, old) in state.refs.iter().filter(|(id, _)| !ids.contains(id.as_str())) {
        refs.push(RefStatus {
            id: id.clone(),
            tracks: crate::sync::tracks(old.kind, &old.name),
            local: Some(old.commit.clone()),
            upstream: None,
            verdict: Verdict::Removed,
            copy_mode: Some(old.copy_mode),
            path: format!("{}/{}", project.config.dir, id),
            detail: Some("the next sync deletes it".into()),
        });
    }
    Ok(Report { project: root.display().to_string(), refs })
}
