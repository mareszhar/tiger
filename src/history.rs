//! `history = true`: the copy is the project's own clone with
//! `--filter=blob:none`, so `git log` and `git diff` work in place and file
//! contents are fetched on first look. These clones are not shared through
//! the store: sharing objects would make git read outside the project.

use std::path::Path;

use crate::config::RefEntry;
use crate::copy;
use crate::git;
use crate::remote::{Kind, Resolved};
use crate::util::unique_name;

/// Brings the clone at `<refs_dir>/<id>` to the resolved commit and returns
/// the commit checked out.
pub fn sync(entry: &RefEntry, resolved: &Resolved, refs_dir: &Path) -> Result<String, String> {
    let dest = refs_dir.join(&entry.id);
    // The raw config value: `remote get-url` would apply URL rewrites (insteadOf).
    let is_clone =
        dest.join(".git").is_dir() && git::run(Some(&dest), &["config", "--get", "remote.origin.url"]).is_ok_and(|u| u.trim() == entry.url);
    if is_clone {
        update(&dest, resolved)?;
        return checkout(&dest, resolved);
    }
    // First sync, or the copy was a plain copy or another URL: clone beside it and swap.
    let tmp = refs_dir.join(unique_name(&format!(".tiger-tmp-{}", entry.id)));
    let result = (|| {
        let tmp_str = tmp.to_string_lossy().to_string();
        let mut args = vec!["clone", "--quiet", "--filter=blob:none", "--no-checkout"];
        let branch;
        if resolved.kind != Kind::Commit {
            branch = resolved.name.clone();
            args.extend(["--single-branch", "--branch", &branch]);
        }
        args.extend([entry.url.as_str(), tmp_str.as_str()]);
        git::run(None, &args).map_err(|e| e.to_string())?;
        if resolved.kind == Kind::Commit && !git::has_commit(&tmp.join(".git"), &resolved.commit) {
            git::run(Some(&tmp), &["fetch", "--quiet", "--filter=blob:none", "origin", &resolved.commit]).map_err(|e| e.to_string())?;
        }
        let commit = checkout(&tmp, resolved)?;
        if let Some(old) = copy::replace(&tmp, &dest).map_err(|e| e.to_string())? {
            copy::remove(&old).map_err(|e| e.to_string())?;
        }
        Ok(commit)
    })();
    if result.is_err() {
        let _ = copy::remove(&tmp);
    }
    result
}

fn update(dest: &Path, resolved: &Resolved) -> Result<(), String> {
    if git::has_commit(&dest.join(".git"), &resolved.commit) {
        return Ok(());
    }
    let refspec = match resolved.kind {
        Kind::Branch => format!("+refs/heads/{0}:refs/remotes/origin/{0}", resolved.name),
        Kind::Tag => format!("+refs/tags/{0}:refs/tags/{0}", resolved.name),
        Kind::Commit => resolved.commit.clone(),
    };
    git::run(Some(dest), &["fetch", "--quiet", "--filter=blob:none", "origin", &refspec]).map(|_| ()).map_err(|e| e.to_string())
}

/// Checks out the resolved commit (detached), read-only, and returns it.
fn checkout(dir: &Path, resolved: &Resolved) -> Result<String, String> {
    let target = match resolved.kind {
        _ if git::has_commit(&dir.join(".git"), &resolved.commit) => resolved.commit.clone(),
        Kind::Branch => format!("refs/remotes/origin/{}", resolved.name),
        Kind::Tag => format!("refs/tags/{}", resolved.name),
        Kind::Commit => resolved.commit.clone(),
    };
    let commit = git::run(Some(dir), &["rev-parse", &format!("{target}^{{commit}}")]).map_err(|e| e.to_string())?.trim().to_string();
    git::run(Some(dir), &["-c", "advice.detachedHead=false", "checkout", "--quiet", "--force", "--detach", &commit])
        .map_err(|e| e.to_string())?;
    copy::make_read_only(dir, Some(".git")).map_err(|e| e.to_string())?;
    Ok(commit)
}
