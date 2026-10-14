//! tiger keeps read-only, up-to-date copies of other repos inside your
//! project, so agents can read the source instead of guessing.

mod commands;
mod config;
mod copy;
mod edit;
mod error;
mod gc;
mod git;
mod history;
mod init;
mod output;
mod pointer;
mod progress;
mod promotion;
mod releases;
mod remote;
mod schedule;
mod settings;
mod show;
mod state;
mod status;
mod store;
mod sync;
mod url;
mod util;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::error::{Error, Result};

const AFTER_HELP: &str = "\
Examples:
  tiger init                               create tiger.toml and the refs folder
  tiger init --dir .                       put copies beside the config; leave ignores alone
  tiger add ueberdosis/tiptap --note \"…\"   add a GitHub repo; it follows the default branch
  tiger add git@host:me/x.git --ref v2.1   add a private repo, pinned to a tag
  tiger sync                               fetch only what changed upstream
  tiger show zod                           the commit, date, and subject zod's copy holds
  tiger sync --everywhere                  update every project on this machine
  tiger config set schedule 6h             do that in the background, every 6 hours

tiger.toml:
  dir = \"tiger\"                    the refs folder (default tiger)
                                   \".\" puts copies beside tiger.toml
  propagate = true                 let --everywhere from other projects update this one

  [refs]
  # Notes about a ref are comments above it.
  tiptap = \"https://github.com/ueberdosis/tiptap\"     follows the default branch

  [refs.zod]
  url = \"https://github.com/colinhacks/zod\"           any git URL, HTTPS or SSH
  ref = \"v3.4.2\"                   a branch follows; a tag or 40-char SHA pins
  include = [\"src/**\"]             globs to keep (default: everything)
  exclude = [\"**/*.snap\"]          globs to drop
  history = false                  true: a real clone, so git log works in place
  releases = false                 true: write <id>.releases.md from GitHub Releases
  propagate = true                 false: only this project's own sync updates it

Exit codes: 0 ok · 1 usage or config error · 2 some refs failed (the rest synced)";

#[derive(Parser)]
#[command(
    name = "tiger",
    version,
    about = "Read-only, up-to-date copies of other repos, inside your project, for agents to read.",
    after_long_help = AFTER_HELP,
    arg_required_else_help = true
)]
struct Cli {
    /// Print results as JSON
    #[arg(long, global = true)]
    json: bool,
    /// Print only failures and warnings
    #[arg(long, short, global = true)]
    quiet: bool,
    /// Run as if started in this folder
    #[arg(long, global = true, value_name = "DIR")]
    cwd: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create tiger.toml (with a usage header), the refs folder, and its gitignore line
    Init {
        /// The refs folder (default: the `dir` setting, else tiger)
        #[arg(long)]
        dir: Option<String>,
        /// Don't add the refs folder to .gitignore
        #[arg(long)]
        no_gitignore: bool,
    },
    /// Add a repo as a ref, then sync it
    Add(AddCli),
    /// Remove refs from tiger.toml and delete their copies
    Rm {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
    },
    /// Sync refs: fetch only what changed upstream
    Sync {
        /// Only these refs (default: all)
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Also update every other project on this machine that uses the same repos
        #[arg(long)]
        everywhere: bool,
        /// Re-copy even if unchanged
        #[arg(long)]
        force: bool,
    },
    /// Pin a ref to a tag or commit (default: the commit synced now)
    Pin {
        id: String,
        #[arg(value_name = "TAG|SHA")]
        target: Option<String>,
        #[arg(long)]
        no_sync: bool,
    },
    /// Follow the default branch again
    Unpin {
        id: String,
        #[arg(long)]
        no_sync: bool,
    },
    /// Show what each ref tracks and whether it is behind upstream
    Status {
        /// Only these refs (default: all)
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Don't contact remotes
        #[arg(long)]
        offline: bool,
    },
    /// Show what each ref's copy holds: commit, date, subject, size, other projects using it
    Show {
        /// Only these refs (default: all)
        #[arg(value_name = "ID")]
        ids: Vec<String>,
    },
    /// Show what the shared store holds and which projects use it
    Store,
    /// Prune store data that no project uses
    Gc,
    /// Show or change global settings (dir, store, concurrency, schedule)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Args)]
struct AddCli {
    /// A git URL (HTTPS or SSH), a local path, or GitHub owner/repo
    url: String,
    /// The id, which is also the folder name (default: the repo name)
    #[arg(long = "as", value_name = "ID")]
    id: Option<String>,
    /// A branch to follow, or a tag or 40-char SHA to pin (default: the default branch)
    #[arg(long = "ref", value_name = "BRANCH|TAG|SHA")]
    git_ref: Option<String>,
    /// Written as a comment above the entry
    #[arg(long)]
    note: Option<String>,
    /// Keep only files matching these globs
    #[arg(long, value_name = "GLOB", num_args = 1..)]
    include: Vec<String>,
    /// Drop files matching these globs
    #[arg(long, value_name = "GLOB", num_args = 1..)]
    exclude: Vec<String>,
    /// Make the copy a real clone, so git log works in place
    #[arg(long)]
    history: bool,
    /// Write GitHub release notes next to the copy
    #[arg(long)]
    releases: bool,
    /// Don't let --everywhere from other projects update this ref
    #[arg(long)]
    no_propagate: bool,
    /// Add the entry without syncing it
    #[arg(long)]
    no_sync: bool,
    /// Defer remote validation until sync (requires --no-sync)
    #[arg(long)]
    no_validate: bool,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// List every setting (the default)
    List,
    /// Print one setting
    Get { key: String },
    /// Change a setting; `schedule` also installs or removes the background job
    Set { key: String, value: String },
    /// Reset a setting to its default
    Unset { key: String },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return match err.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => ExitCode::from(1),
            };
        }
    };
    let json = cli.json;
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "error": err })).expect("errors serialize"));
            } else {
                eprintln!("{err}");
            }
            ExitCode::from(1)
        }
    }
}

fn print_json<T: Serialize>(value: &T) {
    println!("{}", serde_json::to_string_pretty(value).expect("output serializes"));
}

fn sync_code(outcome: &sync::Outcome) -> u8 {
    if outcome.has_failures() { 2 } else { 0 }
}

fn print_sync(outcome: &sync::Outcome, quiet: bool) {
    for warning in &outcome.warnings {
        output::warn(warning);
    }
    print!("{}", output::sync(outcome, quiet));
}

fn run(cli: Cli) -> Result<u8> {
    let start = match &cli.cwd {
        Some(dir) => dir.canonicalize().map_err(|e| Error::io("open", dir, e))?,
        None => std::env::current_dir().map_err(|e| Error::new("could not read the working directory").context(format!("cause: {e}")))?,
    };
    let (json, quiet) = (cli.json, cli.quiet);

    match cli.command {
        Command::Init { dir, no_gitignore } => {
            let settings = settings::load()?;
            let root = config::find_root(&start).filter(|r| *r == start).unwrap_or_else(|| start.clone());
            let dir = dir.unwrap_or_else(|| settings.dir().to_string());
            let done = init::init(&root, &dir, !no_gitignore)?;
            if json {
                print_json(&done);
            } else if !quiet {
                println!("{} {}", if done.created { "created" } else { "found" }, util::display_path(std::path::Path::new(&done.config)));
                if done.gitignore {
                    println!("added the refs folder to .gitignore");
                } else if done.dir == "." {
                    println!("copies go beside tiger.toml; ignore files are left as they are");
                }
                println!(
                    "\nFor the agent instructions (AGENTS.md, CLAUDE.md, …) in {}:\n\n  {}\n",
                    util::display_path(std::path::Path::new(&done.instructions_from)),
                    done.snippet
                );
                println!("Next: {}", done.next);
            }
            Ok(0)
        }
        Command::Add(a) => {
            let args = commands::AddArgs {
                url: a.url,
                id: a.id,
                git_ref: a.git_ref,
                note: a.note,
                include: a.include,
                exclude: a.exclude,
                history: a.history,
                releases: a.releases,
                no_propagate: a.no_propagate,
                no_sync: a.no_sync,
                no_validate: a.no_validate,
                quiet,
            };
            let added = commands::add(&start, &args)?;
            let code = added.sync.as_ref().map(sync_code).unwrap_or(0);
            if json {
                print_json(&added);
                return Ok(code);
            }
            if !quiet {
                if let Some(init) = &added.initialized {
                    println!("created {}", util::display_path(std::path::Path::new(&init.config)));
                }
                match (&added.added, &added.tracks) {
                    (true, Some(tracks)) => println!("added {}, {tracks}", added.id),
                    (true, None) => println!("added {} · remote validation deferred until sync", added.id),
                    (false, _) => println!("{} is already a ref", added.id),
                }
            }
            if let Some(outcome) = &added.sync {
                print_sync(outcome, quiet);
            }
            Ok(code)
        }
        Command::Rm { ids } => {
            let removed = commands::rm(&start, &ids)?;
            if json {
                print_json(&serde_json::json!({ "removed": removed }));
            } else if !quiet {
                for r in &removed {
                    match &r.deleted {
                        Some(path) => println!("removed {} and deleted {path}", r.id),
                        None => println!("removed {}", r.id),
                    }
                }
                if removed.iter().any(|r| r.deleted.is_some()) {
                    println!("the store keeps their data for other projects; `tiger gc` frees what no project uses");
                }
            }
            Ok(0)
        }
        Command::Sync { ids, everywhere, force } => {
            let outcome = sync::sync(&start, &sync::Options { ids, everywhere, force, quiet })?;
            if json {
                if everywhere {
                    print_json(&outcome);
                } else {
                    let project = outcome.projects.first();
                    print_json(&serde_json::json!({
                        "project": project.map(|p| p.project.clone()),
                        "refs": project.map(|p| p.refs.clone()).unwrap_or_default(),
                        "summary": outcome.summary,
                        "warnings": outcome.warnings,
                        "store": outcome.store,
                        "registered": project.map(|p| p.registered).unwrap_or(false),
                        "promotion": project.map(|p| p.promotion.clone()).unwrap_or_default(),
                    }));
                }
            } else {
                print_sync(&outcome, quiet);
            }
            Ok(sync_code(&outcome))
        }
        Command::Pin { id, target, no_sync } => {
            let pinned = commands::pin(&start, &id, target.as_deref(), no_sync, quiet)?;
            report_pin(&pinned, json, quiet)
        }
        Command::Unpin { id, no_sync } => {
            let pinned = commands::unpin(&start, &id, no_sync, quiet)?;
            report_pin(&pinned, json, quiet)
        }
        Command::Status { ids, offline } => {
            let report = status::status(&start, &ids, offline)?;
            if json {
                print_json(&report);
            } else {
                print!("{}", output::status(&report));
            }
            Ok(0)
        }
        Command::Show { ids } => {
            let report = show::show(&start, &ids)?;
            if json {
                print_json(&report);
            } else {
                print!("{}", output::show(&report));
            }
            Ok(0)
        }
        Command::Store => {
            let report = show::store()?;
            if json {
                print_json(&report);
            } else {
                print!("{}", output::store(&report));
            }
            Ok(0)
        }
        Command::Gc => {
            let done = gc::gc()?;
            if json {
                print_json(&done);
            } else if !quiet {
                for project in &done.forgotten_projects {
                    println!("forgot {} (no tiger.toml)", util::display_path(std::path::Path::new(project)));
                }
                println!(
                    "removed {} tree{} and {} repo{} · store {} → {}",
                    done.removed_trees,
                    if done.removed_trees == 1 { "" } else { "s" },
                    done.removed_repos,
                    if done.removed_repos == 1 { "" } else { "s" },
                    output::bytes(done.bytes_before),
                    output::bytes(done.bytes_after)
                );
            }
            Ok(0)
        }
        Command::Config { action } => config_command(action.unwrap_or(ConfigAction::List), json),
    }
}

fn report_pin(pinned: &commands::Pinned, json: bool, quiet: bool) -> Result<u8> {
    let code = pinned.sync.as_ref().map(sync_code).unwrap_or(0);
    if json {
        print_json(pinned);
        return Ok(code);
    }
    if !quiet {
        match &pinned.git_ref {
            Some(r) => println!("pinned {} to {}", pinned.id, if git::is_full_sha(r) { util::short_sha(r) } else { r }),
            None => println!("{} follows the default branch", pinned.id),
        }
    }
    if let Some(outcome) = &pinned.sync {
        print_sync(outcome, quiet);
    }
    Ok(code)
}

#[derive(Serialize)]
struct SettingView {
    key: &'static str,
    value: String,
    default: bool,
    about: &'static str,
}

fn config_command(action: ConfigAction, json: bool) -> Result<u8> {
    let store_root = |s: &settings::Settings| store::Store::shared_path(s.store.as_deref());
    let settings = match &action {
        ConfigAction::Set { key, value } => {
            let updated = settings::set(key, value)?;
            if key == "schedule" || (key == "store" && updated.schedule_secs().is_some()) {
                apply_schedule(&updated)?;
            }
            updated
        }
        ConfigAction::Unset { key } => {
            let updated = settings::unset(key)?;
            if key == "schedule" {
                apply_schedule(&updated)?;
            }
            updated
        }
        _ => settings::load()?,
    };
    let views: Vec<SettingView> = settings::KEYS
        .iter()
        .map(|(key, default, about)| {
            // The store in effect, when $TIGER_HOME overrides the setting.
            let value = match settings.get(key) {
                None if *key == "store" && std::env::var_os("TIGER_HOME").is_some_and(|v| !v.is_empty()) => {
                    std::env::var("TIGER_HOME").ok().map(|home| format!("{home} (from $TIGER_HOME)"))
                }
                value => value,
            };
            SettingView { key, default: value.is_none(), value: value.unwrap_or_else(|| default.to_string()), about }
        })
        .collect();
    let job = schedule::status(&store_root(&settings)?)?;

    if let ConfigAction::Get { key } = &action {
        let view = views.iter().find(|v| v.key == key).ok_or_else(|| settings::unknown(key))?;
        if json {
            print_json(view);
        } else {
            println!("{}", view.value);
        }
        return Ok(0);
    }
    let path = settings::path()?;
    if json {
        print_json(&serde_json::json!({ "path": path.display().to_string(), "settings": views, "schedule_job": job }));
        return Ok(0);
    }
    if let ConfigAction::Set { key, .. } | ConfigAction::Unset { key } = &action {
        let view = views.iter().find(|v| v.key == key).expect("validated key");
        println!("{} = {}{}", view.key, view.value, if view.default { " (default)" } else { "" });
        if key == "schedule" {
            println!("{}", job_line(&job));
        }
        return Ok(0);
    }
    println!("{}", util::display_path(&path));
    let width = views.iter().map(|v| v.key.len() + v.value.len() + 3).max().unwrap_or(0) + 10;
    for view in &views {
        let left = format!("{} = {}{}", view.key, view.value, if view.default { " (default)" } else { "" });
        println!("  {left:<width$}  {}", view.about);
    }
    println!("{}", job_line(&job));
    Ok(0)
}

fn job_line(job: &schedule::JobStatus) -> String {
    if job.installed {
        format!("background job: installed ({}) · log: {}", job.path, job.log)
    } else {
        "background job: not installed".to_string()
    }
}

fn apply_schedule(settings: &settings::Settings) -> Result<()> {
    let root = store::Store::shared_path(settings.store.as_deref())?;
    match settings.schedule_secs() {
        Some(secs) => schedule::install(secs, &root).map(|_| ()),
        None => schedule::remove(&root).map(|_| ()),
    }
}
