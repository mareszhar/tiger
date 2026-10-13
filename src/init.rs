//! `tiger init`: a commented `tiger.toml`, the gitignore line, and `TIGER.md`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{self, Project};
use crate::error::{Error, IoContext, Result};
use crate::pointer;

/// The usage header: everything an agent opening `tiger.toml` needs.
pub fn header(dir: &str, gitignored: bool) -> String {
    let rule = "─".repeat(69);
    let search = if gitignored {
        format!(
            "# • {dir}/ is gitignored, so project-wide searches skip it. To search a ref,\n#   pass its path: rg \"pattern\" {dir}/<id>\n"
        )
    } else {
        format!("# • To search one ref, pass its path: rg \"pattern\" {dir}/<id>\n")
    };
    format!(
        "# ── tiger {rule}\n\
         # Read-only copies of other repos, kept so agents can read the source\n\
         # instead of guessing. Each ref below is copied to {dir}/<id>/.\n\
         #\n\
         {search}\
         # • Never edit files under {dir}/<id>/: every sync replaces them.\n\
         # • Notes about a ref are plain comments directly above its entry.\n\
         # • A ref with only a URL follows the repo's default branch.\n\
         #\n\
         #   tiger add <url> [--as <id>] [--ref <branch|tag|sha>] [--note \"…\"]\n\
         #   tiger sync [<id>…]     fetch only what changed upstream\n\
         #   tiger status           which refs are behind upstream\n\
         #   tiger show [<id>…]     which commit each copy holds, and when\n\
         #   tiger rm <id> · tiger pin <id> [<tag|sha>] · tiger unpin <id> · tiger help\n\
         # {}\n",
        "─".repeat(78)
    )
}

#[derive(Debug, Serialize)]
pub struct Initialized {
    pub config: String,
    pub created: bool,
    pub gitignore: bool,
    pub snippet: String,
}

/// The line to add to agent instructions (AGENTS.md, CLAUDE.md, …).
pub fn snippet(dir: &str) -> String {
    format!(
        "Read-only copies of repos we depend on live in `{dir}/`, managed by tiger and described in `tiger.toml`. Read the source there before guessing; search one with `rg \"pattern\" {dir}/<id>`. Refresh with `tiger sync`."
    )
}

pub fn init(root: &Path, dir: &str, gitignore: bool) -> Result<Initialized> {
    config::validate_dir(dir).map_err(|reason| Error::new(format!("invalid dir: {reason}")))?;
    let dir = dir.trim_end_matches('/');
    let path = root.join(config::FILE);
    let created = !path.exists();
    let project = if created {
        let text = format!("{}\ndir = \"{dir}\"\n\n[refs]\n", header(dir, gitignore));
        config::parse(&text)?;
        crate::util::write_atomic(&path, text.as_bytes())?;
        Project::load(root)?
    } else {
        Project::load(root)?
    };
    let dir = project.config.dir.clone();
    let refs_dir = project.dir_path();
    std::fs::create_dir_all(&refs_dir).at("create", &refs_dir)?;
    let gitignore_added = gitignore && ensure_ignored(root, &dir)?;
    pointer::write(&project)?;
    Ok(Initialized { config: path.display().to_string(), created, gitignore: gitignore_added, snippet: snippet(&dir) })
}

/// Appends `/<dir>/` to `.gitignore` unless git already ignores the folder.
fn ensure_ignored(root: &Path, dir: &str) -> Result<bool> {
    let ignored_by_git = crate::git::run(Some(root), &["check-ignore", "-q", dir]).is_ok();
    let path = root.join(".gitignore");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let line = format!("/{dir}/");
    let listed = current.lines().map(str::trim).any(|l| l == line || l == format!("{dir}/") || l == format!("/{dir}") || l == dir);
    if ignored_by_git || listed {
        return Ok(false);
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!("# tiger: read-only copies of other repos (see tiger.toml)\n{line}\n"));
    crate::util::write_atomic(&path, text.as_bytes())?;
    Ok(true)
}

/// Where `tiger add` creates a config when none exists: the git root, or `start`.
pub fn default_root(start: &Path) -> PathBuf {
    crate::git::run(Some(start), &["rev-parse", "--show-toplevel"])
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| start.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_valid_toml_comments_within_80_columns() {
        let text = format!("{}\n[refs]\n", header("tiger", true));
        config::parse(&text).unwrap();
        assert!(text.lines().all(|l| l.chars().count() <= 80), "{text}");
    }
}
