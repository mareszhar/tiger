//! Git URLs: shorthand expansion, the store key a URL maps to, and the
//! default id a URL suggests.

/// Expands `owner/repo` to a GitHub HTTPS URL; leaves everything else alone.
pub fn expand(input: &str) -> String {
    let input = input.trim();
    let is_shorthand = input.matches('/').count() == 1
        && !input.contains(':')
        && !input.starts_with(['.', '/', '~'])
        && input.split('/').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
    if is_shorthand { format!("https://github.com/{input}") } else { input.to_string() }
}

/// Host and path of a URL, e.g. `("github.com", "ueberdosis/tiptap")`.
/// Local repositories use the host `local`.
pub fn host_and_path(url: &str) -> (String, String) {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.strip_prefix("file://") {
        ("local".to_string(), rest.to_string())
    } else if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = host.split(':').next().unwrap_or(host);
        (host.to_ascii_lowercase(), path.to_string())
    } else if url.starts_with('/') {
        ("local".to_string(), url.to_string())
    } else if let Some((authority, path)) = url.split_once(':') {
        // scp-like: [user@]host:path
        let host = authority.rsplit('@').next().unwrap_or(authority);
        (host.to_ascii_lowercase(), path.to_string())
    } else {
        ("local".to_string(), url.to_string())
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    (host, path.to_string())
}

/// Hosts whose owner and repo names ignore letter case, so `TanStack/router`
/// and `tanstack/router` are one repo. Elsewhere, case can tell repos apart.
pub const CASE_INSENSITIVE_HOSTS: &[&str] = &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// The store key for a URL: `github.com/ueberdosis/tiptap`. HTTPS and SSH
/// URLs of the same repository share a key, so they share the store; so do
/// spellings that differ only in case on hosts that ignore it.
pub fn store_key(url: &str) -> String {
    let (host, path) = host_and_path(url);
    let mut parts = vec![sanitize(&host)];
    parts.extend(path.split('/').filter(|p| !p.is_empty() && *p != "." && *p != "..").map(sanitize));
    fold_key(&parts.join("/"))
}

/// Lowercases a store key's path on hosts that ignore case. Store folders
/// read back from disk go through this too, so a key compares equal however
/// its folder happens to be spelled.
pub fn fold_key(key: &str) -> String {
    match key.split_once('/') {
        Some((host, path)) if CASE_INSENSITIVE_HOSTS.contains(&host) => format!("{host}/{}", path.to_ascii_lowercase()),
        _ => key.to_string(),
    }
}

fn sanitize(part: &str) -> String {
    let cleaned: String = part.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    if cleaned.starts_with('.') { format!("_{cleaned}") } else { cleaned }
}

/// The id a URL suggests: its last path segment, without `.git`.
pub fn default_id(url: &str) -> String {
    let (_, path) = host_and_path(url);
    path.rsplit('/').next().unwrap_or("").to_string()
}

/// `(owner, repo)` for a github.com URL.
pub fn github_repo(url: &str) -> Option<(String, String)> {
    let (host, path) = host_and_path(url);
    if host != "github.com" {
        return None;
    }
    let mut parts = path.split('/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_github_shorthand_only() {
        assert_eq!(expand("ueberdosis/tiptap"), "https://github.com/ueberdosis/tiptap");
        assert_eq!(expand("git@github.com:me/x.git"), "git@github.com:me/x.git");
        assert_eq!(expand("/abs/path"), "/abs/path");
        assert_eq!(expand("./rel/path"), "./rel/path");
    }

    #[test]
    fn https_and_ssh_share_a_key() {
        assert_eq!(store_key("https://github.com/ueberdosis/tiptap"), "github.com/ueberdosis/tiptap");
        assert_eq!(store_key("https://github.com/ueberdosis/tiptap.git/"), "github.com/ueberdosis/tiptap");
        assert_eq!(store_key("git@github.com:ueberdosis/tiptap.git"), "github.com/ueberdosis/tiptap");
        assert_eq!(store_key("ssh://git@GitHub.com:22/ueberdosis/tiptap"), "github.com/ueberdosis/tiptap");
        assert_eq!(store_key("file:///tmp/up"), "local/tmp/up");
        assert_eq!(store_key("https://github.com/TanStack/Router"), store_key("git@github.com:tanstack/router.git"));
        assert_eq!(store_key("https://git.example.com/Team/Repo"), "git.example.com/Team/Repo", "case can matter on other hosts");
        assert_eq!(store_key("https://h/a/../.x"), "h/a/_.x");
    }

    #[test]
    fn suggests_ids_and_github_repos() {
        assert_eq!(default_id("https://code.haverbeke.berlin/prosemirror/prosemirror-model"), "prosemirror-model");
        assert_eq!(default_id("git@github.com:me/internal.git"), "internal");
        assert_eq!(github_repo("git@github.com:me/internal.git"), Some(("me".into(), "internal".into())));
        assert_eq!(github_repo("https://gitlab.com/me/x"), None);
    }
}
