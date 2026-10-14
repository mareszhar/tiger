//! Sync: make every copy match its config, fetching only what changed.
//!
//! One run may cover one project or, with `--everywhere`, every registered
//! project. Across all of them, each URL is asked once (`ls-remote`) and each
//! commit is fetched and extracted once.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::config::{self, Project, RefEntry};
use crate::copy::{self, CopyMode, Filters};
use crate::error::{Error, Result};
use crate::git::{self, GitError};
use crate::remote::{self, Kind, RemoteRefs, Resolved, Want};
use crate::settings::{self, Settings};
use crate::state::{self, RefState, State};
use crate::store::Store;
use crate::url::store_key;
use crate::util::{display_path, now_secs, parallel_map, unique_name};
use crate::{history, pointer, releases};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub ids: Vec<String>,
    pub everywhere: bool,
    pub force: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    New,
    Changed,
    Unchanged,
    Failed,
    Removed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefReport {
    pub id: String,
    pub status: Status,
    /// The branch or tag followed, or the pinned commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Summary {
    pub synced: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub removed: usize,
    pub ms: u128,
}

impl Summary {
    fn add(&mut self, report: &RefReport) {
        match report.status {
            Status::New | Status::Changed => self.synced += 1,
            Status::Unchanged => self.unchanged += 1,
            Status::Failed => self.failed += 1,
            Status::Removed => self.removed += 1,
        }
        // A copy that synced but whose release notes failed counts in both.
        if report.status != Status::Failed && report.error.is_some() {
            self.failed += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectReport {
    pub project: String,
    pub refs: Vec<RefReport>,
    pub summary: Summary,
    pub promotion: crate::promotion::Report,
    pub registered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub projects: Vec<ProjectReport>,
    pub summary: Summary,
    pub store: crate::store::StoreLocation,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub everywhere: bool,
}

impl Outcome {
    pub fn has_failures(&self) -> bool {
        self.summary.failed > 0
            || self
                .projects
                .iter()
                .any(|p| p.error.is_some() || !p.promotion.failures.is_empty() || p.refs.iter().any(|r| r.error.is_some()))
    }
}

/// One project in this run, with the refs it syncs.
struct Run {
    project: Project,
    is_current: bool,
    entries: Vec<RefEntry>,
    /// Whether refs removed from the config are deleted (not when syncing only some ids).
    prune: bool,
    state: State,
    promotion: crate::promotion::Report,
    _lock: Option<std::fs::File>,
}

/// One ref in one project, and what is known about it so far.
struct Job {
    run: usize,
    entry: RefEntry,
    old: Option<RefState>,
    dest: PathBuf,
    folder_exists: bool,
    outcome: JobOutcome,
}

enum JobOutcome {
    Pending,
    Unchanged,
    Resolved(Resolved),
    Done { resolved: Resolved, mode: CopyMode },
    Failed(String),
}

pub fn sync(start: &Path, options: &Options) -> Result<Outcome> {
    let started = Instant::now();
    let progress = crate::progress::Progress::start(options.quiet);
    let activity = &progress.activity;
    let settings = settings::load()?;
    let mut warnings = Vec::new();

    let current = match config::find_root(start) {
        Some(root) => Some(Project::load(&root)?),
        None if options.everywhere => None,
        None => return Err(no_config(start)),
    };
    if let Some(project) = &current {
        check_ids(project, &options.ids)?;
    } else if !options.ids.is_empty() {
        return Err(no_config(start));
    }

    let store = open_store(&settings, current.as_ref(), options.everywhere, &mut warnings)?;
    if !store.shared {
        activity.message(&format!("the shared store isn't writable; using {}", display_path(&store.root)));
    }

    let mut runs = Vec::new();
    let mut reports_by_root: BTreeMap<PathBuf, ProjectReport> = BTreeMap::new();
    let mut missing = Vec::new();
    let wanted_keys: Option<HashSet<String>> = current
        .as_ref()
        .filter(|_| !options.ids.is_empty())
        .map(|p| p.config.refs.iter().filter(|e| options.ids.contains(&e.id)).map(|e| store_key(&e.url)).collect());

    if let Some(project) = current {
        let entries = project.config.refs.iter().filter(|e| options.ids.is_empty() || options.ids.contains(&e.id)).cloned().collect();
        runs.push(Run {
            project,
            is_current: true,
            entries,
            prune: options.ids.is_empty(),
            state: State::default(),
            promotion: Default::default(),
            _lock: None,
        });
    }
    if options.everywhere {
        let registry = store.read_registry()?;
        for root in registry.projects.keys().map(PathBuf::from) {
            if runs.iter().any(|r| same_path(&r.project.root, &root)) {
                continue;
            }
            if !root.join(config::FILE).is_file() {
                missing.push(root);
                continue;
            }
            match Project::load(&root) {
                Ok(project) => {
                    let entries = project
                        .config
                        .refs
                        .iter()
                        .filter(|e| e.propagate && wanted_keys.as_ref().is_none_or(|keys| keys.contains(&store_key(&e.url))))
                        .cloned()
                        .collect();
                    runs.push(Run {
                        project,
                        is_current: false,
                        entries,
                        prune: options.ids.is_empty(),
                        state: State::default(),
                        promotion: Default::default(),
                        _lock: None,
                    });
                }
                Err(err) => {
                    reports_by_root.insert(
                        root.clone(),
                        ProjectReport {
                            project: root.display().to_string(),
                            refs: Vec::new(),
                            summary: Summary { failed: 1, ..Summary::default() },
                            promotion: Default::default(),
                            registered: true,
                            error: Some(err),
                        },
                    );
                }
            }
        }
    }

    // Lock projects in path order so concurrent runs never deadlock.
    activity.begin(runs.iter().map(|run| run.entries.len()).sum(), runs.len());
    let sources: Vec<_> = runs.iter().map(|run| fallback_source(&store, &run.project)).collect();
    let _store_locks = lock_stores(&store, &sources, activity)?;
    activity.quietly("waiting for another tiger process to finish", 0);
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|a, b| runs[*a].project.root.cmp(&runs[*b].project.root));
    for i in order {
        let dir = runs[i].project.dir_path();
        let wait = || activity.message(&format!("waiting for another tiger process to finish with {}", display_path(&dir)));
        match state::lock(&dir, &wait).and_then(|lock| Ok((lock, state::load(&dir)?))) {
            Ok((lock, state)) => {
                runs[i]._lock = Some(lock);
                runs[i].state = state;
            }
            Err(err) if runs[i].is_current => return Err(err),
            Err(err) => {
                let root = runs[i].project.root.clone();
                runs[i].entries.clear();
                runs[i].prune = false;
                reports_by_root.insert(
                    root.clone(),
                    ProjectReport {
                        project: root.display().to_string(),
                        refs: Vec::new(),
                        summary: Summary { failed: 1, ..Summary::default() },
                        promotion: Default::default(),
                        registered: store.shared,
                        error: Some(err),
                    },
                );
            }
        }
        activity.done();
    }

    if store.shared {
        for (run, source) in runs.iter_mut().zip(&sources).filter(|(run, _)| run._lock.is_some()) {
            if let Some(source) = source {
                run.promotion = crate::promotion::import(source, &store, &run.state, &run.entries, activity);
            }
        }
    }
    let mut jobs = plan(&runs, &store, options.force);
    resolve_all(&mut jobs, settings.concurrency(), options.force, activity);
    materialize_all(&mut jobs, &runs, &store, settings.concurrency(), activity);
    relink_copies(&jobs, &mut runs, &store, activity);

    // Release notes for every ref that asks for them and didn't fail.
    activity.quietly("checking release notes", jobs.len());
    let release_results = parallel_map(&jobs, settings.concurrency(), |job| {
        let result = if !job.entry.releases || matches!(job.outcome, JobOutcome::Failed(_)) {
            None
        } else {
            let dir = runs[job.run].project.dir_path();
            Some(releases::refresh(&job.entry, &dir, job.old.as_ref().and_then(|o| o.tags_digest.as_deref())))
        };
        activity.done();
        result
    });

    let mut copy_warned = HashSet::new();
    let mut reports: Vec<Vec<RefReport>> = runs.iter().map(|_| Vec::new()).collect();
    for (job, release) in jobs.into_iter().zip(release_results) {
        let run = &mut runs[job.run];
        let rel_path = rel(&run.project.root, &job.dest);
        let mut report = RefReport {
            id: job.entry.id.clone(),
            status: Status::Unchanged,
            tracks: None,
            from: None,
            to: None,
            path: rel_path,
            error: None,
            warnings: Vec::new(),
        };
        let (mut tags_digest, release_error) = match release {
            Some(Ok(r)) => {
                report.warnings.extend(r.warning);
                (r.digest, None)
            }
            Some(Err(err)) => (job.old.as_ref().and_then(|o| o.tags_digest.clone()), Some(err)),
            None => (None, None),
        };
        if !job.entry.releases {
            tags_digest = None;
            // Release notes turned off: their file goes, as a removed ref's would.
            let notes = run.project.dir_path().join(releases::file_name(&job.entry.id));
            if std::fs::symlink_metadata(&notes).is_ok()
                && let Err(err) = copy::remove(&notes)
            {
                report.warnings.push(format!("could not remove {}: {err}", notes.display()));
            }
        }
        match job.outcome {
            JobOutcome::Done { resolved, mode } => {
                report.status = if job.old.is_some() && job.folder_exists { Status::Changed } else { Status::New };
                report.tracks = Some(tracks(resolved.kind, &resolved.name));
                report.from = job.old.as_ref().filter(|_| job.folder_exists).map(|o| o.commit.clone());
                report.to = Some(resolved.commit.clone());
                if mode == CopyMode::Copy && copy_warned.insert(job.run) {
                    warnings.push(format!("{}: copies are full copies, not copy-on-write clones (the store at {} is on another volume or the filesystem can't clone)", display_path(&run.project.root), display_path(&store.root)));
                }
                run.state.refs.insert(
                    job.entry.id.clone(),
                    RefState {
                        url: job.entry.url.clone(),
                        git_ref: job.entry.git_ref.clone(),
                        kind: resolved.kind,
                        name: resolved.name,
                        commit: resolved.commit,
                        include: job.entry.include.clone(),
                        exclude: job.entry.exclude.clone(),
                        history: job.entry.history,
                        copy_mode: mode,
                        tags_digest,
                        synced_at: now_secs(),
                        checked_at: now_secs(),
                    },
                );
            }
            JobOutcome::Unchanged | JobOutcome::Resolved(_) => {
                let old = job.old.expect("unchanged refs have state");
                report.tracks = Some(tracks(old.kind, &old.name));
                if let Some(st) = run.state.refs.get_mut(&job.entry.id) {
                    st.tags_digest = tags_digest;
                    st.checked_at = now_secs();
                }
            }
            JobOutcome::Failed(err) => {
                report.status = Status::Failed;
                report.tracks = job.old.as_ref().map(|o| tracks(o.kind, &o.name));
                let kept = if job.folder_exists && job.old.is_some() { " · kept existing copy" } else { "" };
                report.error = Some(format!("{err}{kept}"));
            }
            JobOutcome::Pending => unreachable!("every job ends resolved, done, unchanged, or failed"),
        }
        if let Some(err) = release_error {
            report.error = Some(match report.error.take() {
                Some(existing) => format!("{existing}; releases: {err}"),
                None => format!("releases: {err}"),
            });
        }
        reports[job.run].push(report);
    }

    // Removed refs, state files, pointers, registry.
    activity.quietly("saving state", 0);
    let mut registered = Vec::new();
    for (i, run) in runs.iter_mut().enumerate() {
        let dir = run.project.dir_path();
        if run._lock.is_none() {
            continue;
        }
        if run.prune {
            let ids: HashSet<&str> = run.project.config.refs.iter().map(|e| e.id.as_str()).collect();
            let stale: Vec<String> = run.state.refs.keys().filter(|id| !ids.contains(id.as_str())).cloned().collect();
            for id in stale {
                let folder = dir.join(&id);
                let result = copy::remove(&folder).and_then(|_| copy::remove(&dir.join(releases::file_name(&id))));
                let mut report = RefReport {
                    id: id.clone(),
                    status: Status::Removed,
                    tracks: None,
                    from: None,
                    to: None,
                    path: rel(&run.project.root, &folder),
                    error: None,
                    warnings: Vec::new(),
                };
                match result {
                    Ok(()) => {
                        run.state.refs.remove(&id);
                    }
                    Err(err) => {
                        report.status = Status::Failed;
                        report.error = Some(format!("could not remove {}: {err}", folder.display()));
                    }
                }
                reports[i].push(report);
            }
        }
        run.state.last_store = Some(store.location());
        if let Err(err) = state::save(&dir, &run.state).and_then(|_| pointer::write(&run.project)) {
            if run.is_current {
                return Err(err);
            }
            reports_by_root.insert(
                run.project.root.clone(),
                ProjectReport {
                    project: run.project.root.display().to_string(),
                    refs: Vec::new(),
                    summary: Summary { failed: 1, ..Default::default() },
                    promotion: run.promotion.clone(),
                    registered: false,
                    error: Some(err),
                },
            );
            continue;
        }
        registered.push(run.project.root.clone());
        activity.done();
    }
    if store.shared {
        let now = now_secs();
        let missing_keys: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
        store.update_registry(|registry| {
            for key in &missing_keys {
                registry.projects.remove(key);
            }
            for root in &registered {
                registry.projects.insert(root.display().to_string(), crate::store::RegistryEntry { synced_at: now });
            }
        })?;
    }
    for root in &missing {
        warnings.push(format!("forgot {}: it no longer has a {}", display_path(root), config::FILE));
    }
    // An emptied fallback store keeps its lock files, so count only ones still holding data.
    let fallbacks = runs
        .iter()
        .zip(&sources)
        .filter(|(run, source)| {
            registered.contains(&run.project.root)
                && source.as_ref().is_some_and(|s| {
                    !crate::gc::sha_dirs(&s.root.join("trees")).is_empty() || !crate::gc::repo_dirs(&s.root.join("repos")).is_empty()
                })
        })
        .count();
    activity.step("cleaning up the fallback store", fallbacks, "cleaning up the fallback store");
    for (run, source) in runs.iter_mut().zip(&sources) {
        if registered.contains(&run.project.root)
            && let Some(source) = source
        {
            crate::promotion::cleanup(source, &run.state, &run.entries, run.prune, &mut run.promotion);
        }
        activity.done();
    }

    let ms = started.elapsed().as_millis();
    let mut total = Summary { ms, ..Summary::default() };
    let mut projects: Vec<ProjectReport> = Vec::new();
    for (run, refs) in runs.iter().zip(reports) {
        if let Some(report) = reports_by_root.remove(&run.project.root) {
            total.failed += report.summary.failed;
            projects.push(report);
            continue;
        }
        let mut summary = Summary { ms, ..Summary::default() };
        for report in &refs {
            summary.add(report);
            total.add(report);
        }
        projects.push(ProjectReport {
            project: run.project.root.display().to_string(),
            refs,
            summary,
            promotion: run.promotion.clone(),
            registered: store.shared && registered.contains(&run.project.root),
            error: None,
        });
    }
    for (_, report) in reports_by_root {
        total.failed += report.summary.failed;
        projects.push(report);
    }
    Ok(Outcome { projects, summary: total, store: store.location(), warnings, everywhere: options.everywhere })
}

fn fallback_source(destination: &Store, project: &Project) -> Option<Store> {
    if !destination.shared {
        return None;
    }
    let root = project.dir_path().join(crate::store::FALLBACK_DIR).canonicalize().ok()?;
    if !root.is_dir() || same_path(&root, &destination.root) {
        return None;
    }
    Some(Store { root, shared: false })
}

fn lock_stores(destination: &Store, sources: &[Option<Store>], activity: &crate::progress::Activity) -> Result<Vec<std::fs::File>> {
    let mut stores = BTreeMap::new();
    let root = destination.root.canonicalize().unwrap_or_else(|_| destination.root.clone());
    stores.insert(root, true);
    for source in sources.iter().flatten() {
        stores.insert(source.root.clone(), false);
    }
    activity.quietly("waiting for another tiger process to finish", 0);
    let mut locks = Vec::new();
    for (root, shared) in stores {
        let store = Store { root, shared };
        let wait = || activity.message("waiting for another tiger process to finish with the store");
        locks.push(if shared { store.lock_shared(&wait)? } else { store.lock_exclusive(&wait)? });
        activity.done();
    }
    Ok(locks)
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn no_config(start: &Path) -> Error {
    Error::new(format!("no {} in {} or any parent folder", config::FILE, start.display()))
        .hint("run `tiger init` in the project root, or pass --cwd <project>")
}

pub fn check_ids(project: &Project, ids: &[String]) -> Result<()> {
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
    Ok(())
}

pub fn open_store(settings: &Settings, current: Option<&Project>, everywhere: bool, warnings: &mut Vec<String>) -> Result<Store> {
    if let Some(store) = Store::open_shared(settings.store.as_deref())? {
        return Ok(store);
    }
    let shared = Store::shared_path(settings.store.as_deref())?;
    if everywhere {
        return Err(Error::new(format!("can't write the shared store at {}", display_path(&shared)))
            .context("--everywhere updates other projects through the shared store")
            .hint("run it outside the sandbox (your terminal, or `tiger config set schedule 6h`), or allow writes to the store"));
    }
    let project = current.expect("a project exists when not syncing everywhere");
    warnings.push(format!(
        "can't write the shared store at {}; using {}/{} instead (nothing is shared)",
        display_path(&shared),
        project.config.dir,
        crate::store::FALLBACK_DIR
    ));
    Store::open_fallback(&project.dir_path())
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

pub fn tracks(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Commit => crate::util::short_sha(name).to_string(),
        _ => name.to_string(),
    }
}

/// Decides, without the network, which refs are already done.
/// Whether a tag pin's recorded commit is really the tag object's own ID,
/// as tiger 0.1 could record for annotated tags. Such a ref is resolved again.
fn records_tag_object(store: &Store, old: &RefState) -> bool {
    old.kind == Kind::Tag && git::peel(&store.repo_path(&store_key(&old.url)), &old.commit).is_some_and(|commit| commit != old.commit)
}

fn plan(runs: &[Run], store: &Store, force: bool) -> Vec<Job> {
    let mut jobs = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let dir = run.project.dir_path();
        for entry in &run.entries {
            let dest = dir.join(&entry.id);
            let old = run.state.refs.get(&entry.id).cloned();
            let folder_exists = std::fs::symlink_metadata(&dest).is_ok_and(|m| m.is_dir());
            let mut job = Job { run: i, entry: entry.clone(), old, dest, folder_exists, outcome: JobOutcome::Pending };
            if job.old.is_none() && std::fs::symlink_metadata(&job.dest).is_ok() {
                job.outcome = JobOutcome::Failed(format!(
                    "{}/{} already exists and tiger didn't create it; if it's an old copy of this ref, delete it and sync again, otherwise move it or give this ref another id",
                    run.project.config.dir, entry.id
                ));
            } else if !force
                && job.folder_exists
                && job.old.as_ref().is_some_and(|o| o.matches(entry) && o.kind.is_pin() && !records_tag_object(store, o))
            {
                // A synced tag or commit pin needs no network.
                job.outcome = JobOutcome::Unchanged;
            } else if let Want::Commit(sha) = Want::parse(entry.git_ref.as_deref()) {
                job.outcome = JobOutcome::Resolved(Resolved { kind: Kind::Commit, name: sha.clone(), commit: sha, is_default: false });
            }
            jobs.push(job);
        }
    }
    jobs
}

/// One `ls-remote` per URL, with every pattern its refs need.
fn resolve_all(jobs: &mut [Job], concurrency: usize, force: bool, activity: &crate::progress::Activity) {
    let mut patterns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for job in jobs.iter().filter(|j| matches!(j.outcome, JobOutcome::Pending)) {
        patterns.entry(job.entry.url.clone()).or_default().extend(Want::parse(job.entry.git_ref.as_deref()).patterns());
    }
    let urls: Vec<(String, BTreeSet<String>)> = patterns.into_iter().collect();
    activity.step("checking repos upstream", urls.len(), &format!("checking {} upstream", plural(urls.len(), "repo")));
    let answers = parallel_map(&urls, concurrency, |(url, patterns)| {
        let answer =
            remote::ls_remote_with_retry(url, patterns, || activity.message(&format!("{} didn't answer; retrying", store_key(url))));
        activity.done();
        answer
    });
    let answers: HashMap<&str, &std::result::Result<RemoteRefs, GitError>> =
        urls.iter().map(|(u, _)| u.as_str()).zip(answers.iter()).collect();
    for job in jobs.iter_mut().filter(|j| matches!(j.outcome, JobOutcome::Pending)) {
        job.outcome = match answers[job.entry.url.as_str()] {
            Err(err) => JobOutcome::Failed(err.to_string()),
            Ok(remote) => match remote::resolve(&Want::parse(job.entry.git_ref.as_deref()), remote) {
                Ok(resolved) => JobOutcome::Resolved(resolved),
                Err(err) => JobOutcome::Failed(err),
            },
        };
    }
    // Refs whose copy already matches are done.
    for job in jobs.iter_mut().filter(|_| !force) {
        if let JobOutcome::Resolved(resolved) = &job.outcome
            && job.folder_exists
            && job
                .old
                .as_ref()
                .is_some_and(|o| o.matches(&job.entry) && o.commit == resolved.commit && o.kind == resolved.kind && o.name == resolved.name)
        {
            job.outcome = JobOutcome::Unchanged;
        }
    }
}

/// Fetches each needed commit once, then builds every changed copy.
fn materialize_all(jobs: &mut [Job], runs: &[Run], store: &Store, concurrency: usize, activity: &crate::progress::Activity) {
    let mut trees: BTreeMap<(String, String), (String, Option<String>)> = BTreeMap::new();
    for job in jobs.iter() {
        if let JobOutcome::Resolved(r) = &job.outcome
            && !job.entry.history
        {
            trees.entry((store_key(&job.entry.url), r.commit.clone())).or_insert((job.entry.url.clone(), r.fetch_ref()));
        }
    }
    let wanted: Vec<_> = trees.into_iter().collect();
    activity.step("fetching commits", wanted.len(), &format!("fetching {}", plural(wanted.len(), "commit")));
    let fetched = parallel_map(&wanted, concurrency, |((key, sha), (url, fetch_ref))| {
        let result = store.ensure_tree(url, key, fetch_ref.as_deref(), sha);
        activity.done();
        result
    });
    let fetched: HashMap<(String, String), std::result::Result<(String, PathBuf), String>> =
        wanted.into_iter().map(|(k, _)| k).zip(fetched).collect();

    let indices: Vec<usize> = (0..jobs.len()).filter(|i| matches!(jobs[*i].outcome, JobOutcome::Resolved(_))).collect();
    activity.step("copying refs", indices.len(), &format!("copying {}", plural(indices.len(), "ref")));
    let results = {
        let jobs: &[Job] = jobs;
        parallel_map(&indices, concurrency, |i| {
            let job = &jobs[*i];
            let JobOutcome::Resolved(resolved) = &job.outcome else { unreachable!() };
            let dir = runs[job.run].project.dir_path();
            let result = if job.entry.history {
                history::sync(&job.entry, resolved, &dir).map(|commit| (Resolved { commit, ..resolved.clone() }, CopyMode::Git))
            } else {
                match &fetched[&(store_key(&job.entry.url), resolved.commit.clone())] {
                    Err(err) => Err(err.clone()),
                    Ok((commit, tree)) => {
                        build_copy(tree, &dir, &job.entry).map(|mode| (Resolved { commit: commit.clone(), ..resolved.clone() }, mode))
                    }
                }
            };
            activity.done();
            result
        })
    };
    for (i, result) in indices.into_iter().zip(results) {
        jobs[i].outcome = match result {
            Ok((resolved, mode)) => JobOutcome::Done { resolved, mode },
            Err(err) => JobOutcome::Failed(err),
        };
    }
}

/// A copy made from a fallback store, of a commit the shared store already
/// had, holds its own disk blocks. Re-clone it from the shared tree so it
/// shares that space. The content is identical, so its status and copied
/// time stay; a failure keeps the old copy and warns.
fn relink_copies(jobs: &[Job], runs: &mut [Run], store: &Store, activity: &crate::progress::Activity) {
    let targets: Vec<&Job> = jobs
        .iter()
        .filter(|job| matches!(job.outcome, JobOutcome::Unchanged) && !job.entry.history && job.folder_exists)
        .filter(|job| job.old.as_ref().is_some_and(|old| runs[job.run].promotion.needs_relink(&store_key(&old.url), &old.commit)))
        .collect();
    if targets.is_empty() {
        return;
    }
    let count = targets.len();
    activity.step(
        "re-cloning copies from the shared store",
        count,
        &format!("re-cloning {count} cop{} from the shared store", if count == 1 { "y" } else { "ies" }),
    );
    for job in targets {
        let old = job.old.as_ref().expect("filtered on old state");
        let run = &mut runs[job.run];
        let tree = store.tree_path(&store_key(&old.url), &old.commit);
        match build_copy(&tree, &run.project.dir_path(), &job.entry) {
            Ok(mode) => {
                run.promotion.relinked += 1;
                if let Some(state) = run.state.refs.get_mut(&job.entry.id) {
                    state.copy_mode = mode;
                }
            }
            Err(err) => run
                .promotion
                .cleanup_warnings
                .push(format!("could not re-clone {} from the shared store, so it keeps its own disk space: {err}", job.entry.id)),
        }
        activity.done();
    }
}

/// Clones the store's tree beside the copy, filters it, and swaps it in.
fn build_copy(tree: &Path, refs_dir: &Path, entry: &RefEntry) -> std::result::Result<CopyMode, String> {
    let filters = Filters::new(&entry.include, &entry.exclude).map_err(|e| e.to_string())?;
    let tmp = refs_dir.join(unique_name(&format!(".tiger-tmp-{}", entry.id)));
    let result = (|| {
        let mode = copy::clone_tree(tree, &tmp)?;
        filters.apply(&tmp)?;
        if let Some(old) = copy::replace(&tmp, &refs_dir.join(&entry.id))? {
            copy::remove(&old)?;
        }
        Ok::<_, std::io::Error>(mode)
    })();
    result.map_err(|err| {
        let _ = copy::remove(&tmp);
        format!("could not write {}/{}: {err}", refs_dir.display(), entry.id)
    })
}
