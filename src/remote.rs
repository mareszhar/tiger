//! What a ref asks for, what the remote has, and how one resolves against
//! the other. One `git ls-remote` per URL answers every ref that uses it.

use std::collections::{BTreeSet, HashMap};

use crate::git::{self, GitError, is_full_sha};

/// What a ref's `ref` field asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    /// No `ref`: the remote's default branch, whatever it is named.
    DefaultBranch,
    Branch(String),
    Tag(String),
    /// A bare name: a branch or a tag, whichever exists (both is an error).
    Name(String),
    Commit(String),
}

impl Want {
    pub fn parse(git_ref: Option<&str>) -> Want {
        match git_ref {
            None => Want::DefaultBranch,
            Some(r) if is_full_sha(r) => Want::Commit(r.to_ascii_lowercase()),
            Some(r) => {
                if let Some(name) = r.strip_prefix("refs/heads/") {
                    Want::Branch(name.to_string())
                } else if let Some(name) = r.strip_prefix("refs/tags/") {
                    Want::Tag(name.to_string())
                } else {
                    Want::Name(r.to_string())
                }
            }
        }
    }

    /// The `ls-remote` patterns that answer this want.
    pub fn patterns(&self) -> Vec<String> {
        match self {
            Want::DefaultBranch => vec!["HEAD".into()],
            Want::Branch(b) => vec![format!("refs/heads/{b}")],
            // `ls-remote` lists the `^{}` line (the commit an annotated tag
            // points at) only when a pattern asks for it by name.
            Want::Tag(t) => vec![format!("refs/tags/{t}"), format!("refs/tags/{t}^{{}}")],
            Want::Name(n) => vec![format!("refs/heads/{n}"), format!("refs/tags/{n}"), format!("refs/tags/{n}^{{}}")],
            Want::Commit(_) => vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Branch,
    Tag,
    Commit,
}

impl Kind {
    pub fn is_pin(self) -> bool {
        self != Kind::Branch
    }
}

/// A want resolved against the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub kind: Kind,
    /// The branch or tag name, or the commit SHA.
    pub name: String,
    pub commit: String,
    /// True when the branch is the remote's default branch.
    pub is_default: bool,
}

impl Resolved {
    /// The full refname to fetch, or `None` for a commit (fetched by SHA).
    pub fn fetch_ref(&self) -> Option<String> {
        match self.kind {
            Kind::Branch => Some(format!("refs/heads/{}", self.name)),
            Kind::Tag => Some(format!("refs/tags/{}", self.name)),
            Kind::Commit => None,
        }
    }
}

/// What `ls-remote` reported for one URL.
#[derive(Debug, Clone, Default)]
pub struct RemoteRefs {
    pub head: Option<String>,
    /// Full refname → commit (tags peeled to the commit they point at).
    pub refs: HashMap<String, String>,
}

pub fn ls_remote(url: &str, patterns: &BTreeSet<String>) -> Result<RemoteRefs, GitError> {
    ls_remote_with_retry(url, patterns, || {})
}

pub fn ls_remote_with_retry(url: &str, patterns: &BTreeSet<String>, retry: impl Fn()) -> Result<RemoteRefs, GitError> {
    let mut args = vec!["ls-remote", "--symref", url];
    args.extend(patterns.iter().map(String::as_str));
    // A stalled connection usually succeeds at once on a fresh one.
    let out = match git::run(None, &args) {
        Err(err) if err.0.starts_with(git::TIMED_OUT) => {
            retry();
            git::run(None, &args)?
        }
        other => other?,
    };
    Ok(parse_ls_remote(&out))
}

pub fn parse_ls_remote(out: &str) -> RemoteRefs {
    let mut remote = RemoteRefs::default();
    let mut peeled: HashMap<String, String> = HashMap::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("ref: ") {
            if let Some((target, name)) = rest.split_once('\t')
                && name == "HEAD"
            {
                remote.head = Some(target.to_string());
            }
            continue;
        }
        let Some((sha, name)) = line.split_once('\t') else { continue };
        if let Some(base) = name.strip_suffix("^{}") {
            peeled.insert(base.to_string(), sha.to_string());
        } else {
            remote.refs.insert(name.to_string(), sha.to_string());
        }
    }
    for (name, sha) in peeled {
        remote.refs.insert(name, sha);
    }
    remote
}

pub fn resolve(want: &Want, remote: &RemoteRefs) -> Result<Resolved, String> {
    let branch = |name: &str, is_default: bool| {
        remote.refs.get(&format!("refs/heads/{name}")).map(|sha| Resolved {
            kind: Kind::Branch,
            name: name.to_string(),
            commit: sha.clone(),
            is_default,
        })
    };
    let tag = |name: &str| {
        remote.refs.get(&format!("refs/tags/{name}")).map(|sha| Resolved {
            kind: Kind::Tag,
            name: name.to_string(),
            commit: sha.clone(),
            is_default: false,
        })
    };
    match want {
        Want::DefaultBranch => {
            let head = remote.head.as_deref().ok_or("the remote has no default branch (HEAD); set ref to a branch, tag, or commit")?;
            let name = head.strip_prefix("refs/heads/").unwrap_or(head);
            match remote.refs.get("HEAD").or_else(|| remote.refs.get(head)) {
                Some(sha) => Ok(Resolved { kind: Kind::Branch, name: name.to_string(), commit: sha.clone(), is_default: true }),
                None => Err(format!("the remote's default branch '{name}' has no commits")),
            }
        }
        Want::Branch(name) => branch(name, false).ok_or_else(|| format!("no branch '{name}' on the remote")),
        Want::Tag(name) => tag(name).ok_or_else(|| format!("no tag '{name}' on the remote")),
        Want::Name(name) => match (branch(name, false), tag(name)) {
            (Some(_), Some(_)) => Err(format!(
                "'{name}' is both a branch and a tag; set ref to \"refs/heads/{name}\" to follow the branch or \"refs/tags/{name}\" to pin the tag"
            )),
            (Some(b), None) => Ok(b),
            (None, Some(t)) => Ok(t),
            (None, None) => Err(format!("no branch or tag '{name}' on the remote")),
        },
        Want::Commit(sha) => Ok(Resolved { kind: Kind::Commit, name: sha.clone(), commit: sha.clone(), is_default: false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "ref: refs/heads/trunk\tHEAD\n\
        aaaa\tHEAD\n\
        bbbb\trefs/heads/v1\n\
        cccc\trefs/tags/v1\n\
        dddd\trefs/tags/v1^{}\n\
        eeee\trefs/tags/v2\n";

    #[test]
    fn resolves_default_branch_tags_and_clashes() {
        let remote = parse_ls_remote(OUT);
        let default = resolve(&Want::DefaultBranch, &remote).unwrap();
        assert_eq!((default.kind, default.name.as_str(), default.commit.as_str()), (Kind::Branch, "trunk", "aaaa"));
        assert!(resolve(&Want::parse(Some("v1")), &remote).unwrap_err().contains("both a branch and a tag"));
        let tag = resolve(&Want::parse(Some("refs/tags/v1")), &remote).unwrap();
        assert_eq!((tag.kind, tag.commit.as_str()), (Kind::Tag, "dddd"), "annotated tags resolve to their commit");
        assert_eq!(resolve(&Want::parse(Some("v2")), &remote).unwrap().kind, Kind::Tag);
        assert!(resolve(&Want::parse(Some("nope")), &remote).is_err());
    }

    #[test]
    fn parses_wants() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(Want::parse(Some(sha)), Want::Commit(sha.into()));
        assert_eq!(Want::parse(Some("refs/heads/main")), Want::Branch("main".into()));
        assert_eq!(Want::parse(None), Want::DefaultBranch);
    }
}
