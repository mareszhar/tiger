//! `releases = true`: `<id>.releases.md` from the GitHub Releases API,
//! newest first. The API is called only when the repo's tag list changed.

use std::path::Path;
use std::time::Duration;

use crate::config::RefEntry;
use crate::git;
use crate::url::github_repo;
use crate::util::{digest, write_atomic};

const MAX_PAGES: usize = 10;

pub fn file_name(id: &str) -> String {
    format!("{id}.releases.md")
}

pub struct Refreshed {
    /// Digest of the tag list the notes were written from.
    pub digest: Option<String>,
    pub warning: Option<String>,
}

pub fn refresh(entry: &RefEntry, refs_dir: &Path, old_digest: Option<&str>) -> Result<Refreshed, String> {
    let Some((owner, repo)) = github_repo(&entry.url) else {
        return Ok(Refreshed { digest: None, warning: Some("releases: only GitHub repos are supported; skipped".into()) });
    };
    let tags = git::run(None, &["ls-remote", "--tags", "--refs", &entry.url]).map_err(|e| e.to_string())?;
    let tags_digest = digest(tags.as_bytes());
    let path = refs_dir.join(file_name(&entry.id));
    if old_digest == Some(tags_digest.as_str()) && path.is_file() {
        return Ok(Refreshed { digest: Some(tags_digest), warning: None });
    }
    let releases = fetch(&owner, &repo)?;
    write_atomic(&path, render(&owner, &repo, &releases).as_bytes()).map_err(|e| e.message)?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444));
    Ok(Refreshed { digest: Some(tags_digest), warning: None })
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
}

fn token() -> Option<String> {
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(token) = std::env::var(var)
            && !token.trim().is_empty()
        {
            return Some(token.trim().to_string());
        }
    }
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !token.is_empty()).then_some(token)
}

fn fetch(owner: &str, repo: &str) -> Result<Vec<Release>, String> {
    let base = std::env::var("TIGER_GITHUB_API").unwrap_or_else(|_| "https://api.github.com".into());
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(30))).http_status_as_error(false).build().into();
    let token = token();
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = format!("{base}/repos/{owner}/{repo}/releases?per_page=100&page={page}");
        let mut request = agent
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", concat!("tiger/", env!("CARGO_PKG_VERSION")));
        if let Some(token) = &token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request.call().map_err(|e| format!("GitHub API: {e}"))?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().map_err(|e| format!("GitHub API: {e}"))?;
        match status {
            200 => {}
            403 | 429 if token.is_none() => {
                return Err("GitHub API rate limit reached; set GITHUB_TOKEN or sign in with `gh auth login`".into());
            }
            404 => return Err(format!("GitHub API: {owner}/{repo} not found (private repos need GITHUB_TOKEN or `gh auth login`)")),
            _ => return Err(format!("GitHub API answered {status}")),
        }
        let releases: Vec<Release> = serde_json::from_str(&body).map_err(|e| format!("GitHub API: unexpected response: {e}"))?;
        let done = releases.len() < 100;
        all.extend(releases.into_iter().filter(|r| !r.draft));
        if done {
            break;
        }
    }
    Ok(all)
}

fn render(owner: &str, repo: &str, releases: &[Release]) -> String {
    let mut out = format!(
        "# {owner}/{repo} releases\n\nNewest first, from GitHub Releases. Written by tiger; replaced when the repo's tags change.\n"
    );
    if releases.is_empty() {
        out.push_str("\nNo releases published.\n");
    }
    for release in releases {
        let mut heading = release.tag_name.clone();
        if let Some(name) = release
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty() && *n != release.tag_name && Some(*n) != release.tag_name.strip_prefix('v'))
        {
            heading.push_str(&format!(" · {name}"));
        }
        if let Some(date) = release.published_at.as_deref().and_then(|d| d.get(..10)) {
            heading.push_str(&format!(" — {date}"));
        }
        if release.prerelease {
            heading.push_str(" (pre-release)");
        }
        out.push_str(&format!("\n## {heading}\n"));
        let body = release.body.as_deref().unwrap_or("").replace("\r\n", "\n");
        let body = body.trim();
        if !body.is_empty() {
            out.push_str(&format!("\n{}\n", demote_headings(body)));
        }
    }
    out
}

/// Pushes a release body's headings two levels down, so they nest under the
/// release's own `##` heading. Code blocks are left alone.
fn demote_headings(body: &str) -> String {
    let mut fence: Option<&str> = None;
    let mut out = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
            fence = match fence {
                Some(open) if open == marker => None,
                None => Some(marker),
                other => other,
            };
            out.push(line.to_string());
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if fence.is_none() && (1..=6).contains(&level) && line[level..].starts_with(' ') {
            out.push(format!("{}{}", "#".repeat((level + 2).min(6)), &line[level..]));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_newest_first_without_drafts() {
        let releases: Vec<Release> = serde_json::from_str(
            r#"[{"tag_name":"v2.0.0","name":"Big one","body":"Breaking.\r\n","prerelease":true,"published_at":"2025-05-01T10:00:00Z"},
                {"tag_name":"v1.0.0","name":"v1.0.0","body":null,"published_at":"2024-01-01T00:00:00Z"}]"#,
        )
        .unwrap();
        let text = render("o", "r", &releases);
        assert!(text.contains("## v2.0.0 · Big one — 2025-05-01 (pre-release)\n\nBreaking.\n"), "{text}");
        assert!(text.contains("## v1.0.0 — 2024-01-01\n"), "{text}");
        assert!(text.find("v2.0.0").unwrap() < text.find("v1.0.0").unwrap());
    }

    #[test]
    fn nests_body_headings_under_the_release() {
        let body = "## Features\n- x\n```sh\n# a comment\n```\n###### deep";
        assert_eq!(demote_headings(body), "#### Features\n- x\n```sh\n# a comment\n```\n###### deep");
    }
}
