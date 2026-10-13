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
