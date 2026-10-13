//! Plain-text rendering. JSON output is the same data, serialized.

use std::fmt::Write;
use std::path::Path;

use crate::status::{self, Verdict};
use crate::sync::{Outcome, ProjectReport, RefReport, Status};
use crate::util::{display_path, format_duration_ms, short_sha};

pub fn warn(message: &str) {
    eprintln!("tiger: warning: {message}");
}

fn ref_line(out: &mut String, report: &RefReport, width: usize) {
    let id = format!("{:<width$}", report.id);
    let tracks = report.tracks.as_deref().unwrap_or("");
    let _ = match report.status {
        Status::New => writeln!(out, "  + new        {id}  {tracks} → {}", report.to.as_deref().map(short_sha).unwrap_or("")),
        Status::Changed => match (&report.from, &report.to) {
            (Some(from), Some(to)) if from == to => writeln!(out, "  ~ rebuilt    {id}  {tracks} {}", short_sha(to)),
            (from, to) => writeln!(
                out,
                "  ~ changed    {id}  {tracks} {} → {}",
                from.as_deref().map(short_sha).unwrap_or("?"),
                to.as_deref().map(short_sha).unwrap_or("?")
            ),
        },
        Status::Unchanged => writeln!(out, "  = unchanged  {id}  {tracks}"),
        Status::Failed => writeln!(out, "  ! failed     {id}  {}", report.error.as_deref().unwrap_or("")),
        Status::Removed => writeln!(out, "  - removed    {id}"),
    };
    if report.status != Status::Failed
        && let Some(err) = &report.error
    {
        let _ = writeln!(out, "  ! failed     {id}  {err}");
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "    {warning}");
    }
}

fn project_lines(out: &mut String, project: &ProjectReport, quiet: bool, indent_unchanged: bool) -> bool {
    if let Some(err) = &project.error {
        for (i, line) in err.to_string().lines().enumerate() {
            let _ = writeln!(out, "  {}{line}", if i == 0 { "! " } else { "  " });
        }
        return true;
    }
    let width = project.refs.iter().map(|r| r.id.len()).max().unwrap_or(0).max(8);
    let mut wrote = false;
    for report in &project.refs {
        let failed = report.status == Status::Failed || report.error.is_some();
        if report.status == Status::Unchanged && report.error.is_none() && report.warnings.is_empty() {
            continue;
        }
        if quiet && !failed {
            continue;
        }
        ref_line(out, report, width);
        wrote = true;
    }
    let unchanged = project.refs.iter().filter(|r| r.status == Status::Unchanged && r.error.is_none() && r.warnings.is_empty()).count();
    if unchanged > 0 && !quiet && indent_unchanged {
        let _ = writeln!(out, "  = unchanged  {unchanged} ref{}", if unchanged == 1 { "" } else { "s" });
        wrote = true;
    }
    wrote
}

fn summary_line(outcome: &Outcome) -> String {
    let s = &outcome.summary;
    let mut line = format!("synced {} · unchanged {} · failed {}", s.synced, s.unchanged, s.failed);
    if s.removed > 0 {
        line.push_str(&format!(" · removed {}", s.removed));
    }
    if outcome.everywhere {
        line.push_str(&format!(" · {} project{}", outcome.projects.len(), if outcome.projects.len() == 1 { "" } else { "s" }));
    }
    line.push_str(&format!(" · {}", format_duration_ms(s.ms)));
    line
}

pub fn sync(outcome: &Outcome, quiet: bool) -> String {
    let mut out = String::new();
    if outcome.everywhere {
        for project in &outcome.projects {
            let mut body = String::new();
            if project_lines(&mut body, project, quiet, false) {
                let _ = writeln!(out, "{}", display_path(Path::new(&project.project)));
                out.push_str(&body);
            }
        }
    } else if let Some(project) = outcome.projects.first() {
        project_lines(&mut out, project, quiet, true);
    }
    if !quiet {
        out.push_str(&summary_line(outcome));
        out.push('\n');
    }
    out
}

pub fn status(report: &status::Report) -> String {
    let mut out = String::new();
    if report.refs.is_empty() {
        out.push_str("no refs yet; add one with `tiger add <url>`\n");
        return out;
    }
    let id_w = report.refs.iter().map(|r| r.id.len()).max().unwrap_or(0);
    let tracks_w = report.refs.iter().map(|r| r.tracks.chars().count()).max().unwrap_or(0);
    for r in &report.refs {
        let local = r.local.as_deref().map(short_sha).unwrap_or("—");
        let mut verdict = r.verdict.label().to_string();
        if r.verdict == Verdict::Behind
            && let Some(up) = &r.upstream
        {
            verdict.push_str(&format!(" → {}", short_sha(up)));
        }
        if let Some(detail) = &r.detail {
            verdict.push_str(&format!(" · {detail}"));
        }
        if r.copy_mode == Some(crate::copy::CopyMode::Copy) {
            verdict.push_str(" · full copy");
        }
        let tracks = format!("{:<tracks_w$}", r.tracks);
        let _ = writeln!(out, "{:<id_w$}  {tracks}  {local:<7}  {verdict}", r.id);
    }
    out
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max { text.to_string() } else { format!("{}…", text.chars().take(max - 1).collect::<String>()) }
}

fn commit_line(c: &crate::show::CommitInfo) -> String {
    let mut line = short_sha(&c.sha).to_string();
    if !c.date.is_empty() {
        line.push_str(&format!("  {}  {}", c.date, truncate(&c.subject, 60)));
    }
    if !c.author.is_empty() {
        line.push_str(&format!("  ({})", c.author));
    }
    line
}

pub fn show(report: &crate::show::ShowReport) -> String {
    use crate::remote::Kind;
    let mut out = String::new();
    if report.refs.is_empty() {
        out.push_str("no refs yet; add one with `tiger add <url>`\n");
        return out;
    }
    for (i, r) in report.refs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "{}  {}", r.id, r.url);
        if !r.synced {
            let _ = writeln!(out, "  not synced yet: run `tiger sync {}`", r.id);
            continue;
        }
        let tracks = r.tracks.as_deref().unwrap_or("");
        let tracks = match (r.kind, &r.git_ref) {
            (Some(Kind::Branch), None) => format!("{tracks} (default branch)"),
            (Some(Kind::Branch), Some(_)) => tracks.to_string(),
            (Some(Kind::Tag), _) => format!("tag {tracks} (pinned)"),
            _ => format!("commit {tracks} (pinned)"),
        };
        let _ = writeln!(out, "  tracks    {tracks}");
        if let Some(c) = &r.commit {
            let _ = writeln!(out, "  commit    {}", commit_line(c));
        }
        if let Some(at) = r.copied_at {
            let checked = r.checked_at.map(|c| format!(" · checked {}", crate::util::ago(c))).unwrap_or_default();
            let _ = writeln!(out, "  copied    {} ({}){checked}", crate::util::format_time(at), crate::util::ago(at));
        }
        let mode = match r.copy_mode {
            Some(crate::copy::CopyMode::Clone) => "copy-on-write clone",
            Some(crate::copy::CopyMode::Copy) => "full copy",
            Some(crate::copy::CopyMode::Git) => "git clone with history",
            None => "",
        };
        let _ =
            writeln!(out, "  copy      {} · {} files · {} · {mode}", r.path, thousands(r.files.unwrap_or(0)), bytes(r.bytes.unwrap_or(0)));
        if !r.include.is_empty() || !r.exclude.is_empty() {
            let mut parts = Vec::new();
            if !r.include.is_empty() {
                parts.push(format!("include {}", r.include.join(", ")));
            }
            if !r.exclude.is_empty() {
                parts.push(format!("exclude {}", r.exclude.join(", ")));
            }
            let _ = writeln!(out, "  filters   {}", parts.join(" · "));
        }
        if let Some(notes) = &r.release_notes {
            let _ = writeln!(out, "  releases  {notes}");
        }
        if !r.elsewhere.is_empty() {
            let list: Vec<String> = r
                .elsewhere
                .iter()
                .map(|e| format!("{} as {} ({})", display_path(Path::new(&e.project)), e.id, short_sha(&e.commit)))
                .collect();
            let _ = writeln!(out, "  also in   {}", list.join(" · "));
        }
        if r.config_changed {
            let _ = writeln!(out, "  config changed since this sync: `tiger sync {}` rebuilds it", r.id);
        }
    }
    out
}

pub fn store(report: &crate::show::StoreReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} · {}", display_path(Path::new(&report.path)), bytes(report.bytes));
    let _ = writeln!(out, "projects");
    if report.projects.is_empty() {
        let _ = writeln!(out, "  (none yet: a project registers on its first sync)");
    }
    let width = report.projects.iter().map(|p| display_path(Path::new(&p.project)).chars().count()).max().unwrap_or(0);
    for p in &report.projects {
        let path = format!("{:<width$}", display_path(Path::new(&p.project)));
        if p.exists {
            let _ =
                writeln!(out, "  {path}  {} ref{} · synced {}", p.refs, if p.refs == 1 { "" } else { "s" }, crate::util::ago(p.synced_at));
        } else {
            let _ = writeln!(out, "  {path}  missing: `tiger gc` forgets it");
        }
    }
    let _ = writeln!(out, "repos");
    if report.repos.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for repo in &report.repos {
        let _ = writeln!(out, "  {} · {}", repo.key, bytes(repo.bytes));
        for tree in &repo.trees {
            let users = if tree.used_by.is_empty() {
                "unused: `tiger gc` removes it".to_string()
            } else {
                tree.used_by.iter().map(|u| format!("{} ({})", display_path(Path::new(&u.project)), u.id)).collect::<Vec<_>>().join(", ")
            };
            let date = if tree.commit.date.is_empty() { String::new() } else { format!("  {}", tree.commit.date) };
            let _ = writeln!(out, "    {}{date}  {}  →  {users}", short_sha(&tree.commit.sha), truncate(&tree.commit.subject, 50));
        }
    }
    out
}
