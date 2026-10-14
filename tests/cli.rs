//! End-to-end tests: the real binary against real git repositories.
//!
//! Upstreams are local repositories reached through `file://` URLs, so the
//! suite runs offline. Every test gets its own store, settings, and home,
//! and git ignores the machine's global config, so nothing outside the
//! temporary folder is read or written.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Env {
    tmp: tempfile::TempDir,
}

fn hermetic(cmd: &mut Command, root: &Path) {
    cmd.env("HOME", root.join("home"))
        .env("TIGER_HOME", root.join("store"))
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env("TIGER_SCHEDULER_DIR", root.join("jobs"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        Env { tmp }
    }

    fn root(&self) -> &Path {
        self.tmp.path()
    }

    fn store(&self) -> PathBuf {
        self.root().join("store")
    }

    /// A project folder (a git repository, like real projects).
    fn project(&self, name: &str) -> PathBuf {
        let dir = self.root().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q", "-b", "main"]);
        dir
    }

    fn tiger(&self, dir: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
        hermetic(&mut cmd, self.root());
        cmd.current_dir(dir).args(args).output().unwrap()
    }

    fn fallback_json(&self, dir: &Path, args: &[&str]) -> Value {
        let blocked = self.root().join("blocked-store");
        std::fs::write(&blocked, "not a directory").unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
        hermetic(&mut cmd, self.root());
        let out = cmd.current_dir(dir).env("TIGER_HOME", blocked).arg("--json").args(args).output().unwrap();
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Runs tiger and requires the exit code.
    fn ok(&self, dir: &Path, args: &[&str], code: i32) -> String {
        let out = self.tiger(dir, args);
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert_eq!(out.status.code(), Some(code), "tiger {}:\n{text}", args.join(" "));
        text
    }

    fn json(&self, dir: &Path, args: &[&str], code: i32) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.tiger(dir, &full);
        assert_eq!(
            out.status.code(),
            Some(code),
            "tiger {}:\n{}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        hermetic(&mut cmd, self.root());
        let out = cmd.current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// An upstream repository whose default branch is `trunk` (not `main`,
    /// so nothing can assume the name), with one commit.
    fn upstream(&self, name: &str) -> (PathBuf, String) {
        let dir = self.root().join("upstreams").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q", "-b", "trunk"]);
        self.commit(&dir, &[("README.md", "# hello\n"), ("src/lib.ts", "export const a = 1\n")]);
        let url = format!("file://{}", dir.display());
        (dir, url)
    }

    fn commit(&self, repo: &Path, files: &[(&str, &str)]) -> String {
        for (file, text) in files {
            let path = repo.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        self.git(repo, &["add", "-A"]);
        self.git(repo, &["commit", "-q", "-m", "change"]);
        self.git(repo, &["rev-parse", "HEAD"])
    }
}

fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path.as_ref()).unwrap_or_else(|e| panic!("{}: {e}", path.as_ref().display()))
}

fn statuses(json: &Value) -> Vec<(String, String)> {
    json["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["id"].as_str().unwrap().to_string(), r["status"].as_str().unwrap().to_string()))
        .collect()
}

/// Folders named `name` anywhere under `root`.
fn find_dirs(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() == name {
                    found.push(path);
                } else {
                    found.extend(find_dirs(&path, name));
                }
            }
        }
    }
    found
}

fn pair(id: &str, status: &str) -> (String, String) {
    (id.to_string(), status.to_string())
}

#[test]
fn config_and_copies_can_live_side_by_side_without_git_or_ignore_edits() {
    let env = Env::new();
    let (upstream, url) = env.upstream("lib");
    let project = env.root().join("plain folder");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("AGENTS.md"), "My instructions.\n").unwrap();
    std::fs::write(project.join(".gitignore"), "my-own-rule\n").unwrap();
    let init = env.json(&project, &["init", "--dir", "."], 0);
    assert_eq!(init["gitignore"], false);
    assert_eq!(read(project.join(".gitignore")), "my-own-rule\n");
    assert!(read(project.join("TIGER.md")).contains("](tiger.toml)"));
    env.json(&project, &["add", &url, "--as", "lib"], 0);
    assert_eq!(read(project.join("lib/README.md")), "# hello\n");
    env.json(&project, &["pin", "lib", "--no-sync"], 0);
    env.json(&project, &["init", "--dir", "another"], 0);
    assert!(read(project.join("tiger.toml")).contains("dir = \".\""));
    assert!(!project.join("another").exists());
    std::fs::rename(&upstream, env.root().join("unavailable")).unwrap();
    env.json(&project, &["sync"], 0);
    assert_eq!(read(project.join("AGENTS.md")), "My instructions.\n");
    assert!(!project.join(".git").exists());
    env.json(&project, &["rm", "lib"], 0);
    assert!(!project.join("lib").exists());
    assert!(project.join("tiger.toml").is_file());
    assert_eq!(read(project.join("AGENTS.md")), "My instructions.\n");
    for id in ["tiger.toml", "Tiger.Toml", "tiger.MD"] {
        let before = read(project.join("tiger.toml"));
        env.json(&project, &["add", &url, "--as", id, "--no-sync"], 1);
        assert_eq!(read(project.join("tiger.toml")), before);
    }
}

#[test]
fn nested_instructions_select_their_config_even_with_a_root_config() {
    let env = Env::new();
    let root = env.project("app");
    env.json(&root, &["init"], 0);
    let nested = root.join("refs with 'quotes'");
    std::fs::create_dir(&nested).unwrap();
    let init = env.json(&nested, &["init", "--dir", "."], 0);
    assert_eq!(init["instructions_from"], root.canonicalize().unwrap().display().to_string());
    let snippet = init["snippet"].as_str().unwrap();
    assert!(snippet.contains("refs with 'quotes'/tiger.toml"), "{snippet}");
    assert!(!init["next"].as_str().unwrap().contains("--cwd"), "init ran in the nested folder, so its next command runs there");
    let sync_command = snippet.rsplit('`').nth(1).unwrap();
    let mut cmd = Command::new("sh");
    hermetic(&mut cmd, env.root());
    let bin = Path::new(env!("CARGO_BIN_EXE_tiger")).parent().unwrap();
    cmd.env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()));
    let out = cmd.current_dir(&root).args(["-c", sync_command]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(nested.join(".tiger-state.json").is_file());
    assert!(!root.join("tiger/.tiger-state.json").exists());
    let (_, url) = env.upstream("lib");
    let next = init["next"].as_str().unwrap().replace("<url|owner/repo>", &url);
    let mut cmd = Command::new("sh");
    hermetic(&mut cmd, env.root());
    cmd.env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()));
    let out = cmd.current_dir(&nested).args(["-c", &next]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(nested.join("lib/README.md").is_file());
    assert!(!read(root.join("tiger.toml")).contains(&url));
    env.json(&nested, &["add", &url, "--as", "second", "--no-sync"], 0);
    // Run from the Git root with --cwd, a hint must select the nested config from there.
    let show = env.ok(&root, &["--cwd", "refs with 'quotes'", "show", "second"], 0);
    let hint = show.lines().find(|line| line.contains("not synced yet:")).unwrap().split('`').nth(1).unwrap();
    assert!(hint.contains("--cwd"), "{hint}");
    let mut cmd = Command::new("sh");
    hermetic(&mut cmd, env.root());
    cmd.env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()));
    let out = cmd.current_dir(&root).args(["-c", hint]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(nested.join("second/README.md").is_file());
}

#[test]
fn nested_configs_have_separate_refs_folders_and_never_share_ownership_state() {
    let env = Env::new();
    let root = env.project("app");
    env.json(&root, &["init", "--dir", "refs"], 0);
    let (_, url) = env.upstream("lib");
    env.json(&root, &["add", &url], 0);
    let nested = root.join("refs");
    let state = read(nested.join(".tiger-state.json"));
    env.json(&nested, &["init", "--dir", "."], 1);
    assert!(!nested.join("tiger.toml").exists());
    assert_eq!(read(nested.join(".tiger-state.json")), state);
    std::fs::write(nested.join("tiger.toml"), "dir = \".\"\n[refs]\n").unwrap();
    env.json(&root, &["sync"], 1);
    env.json(&nested, &["sync"], 1);
    assert_eq!(read(nested.join(".tiger-state.json")), state);
    assert!(nested.join("lib/README.md").is_file());
    std::fs::write(nested.join("tiger.toml"), "dir = \"separate\"\n[refs]\n").unwrap();
    env.json(&nested, &["sync"], 0);
    env.json(&root, &["sync", "--everywhere"], 0);
    assert!(nested.join("separate/.tiger-state.json").is_file());
    assert!(nested.join("lib/README.md").is_file());
}

#[test]
fn invalid_ownership_ids_cannot_remove_the_config_beside_copies() {
    let env = Env::new();
    let project = env.project("app");
    env.json(&project, &["init", "--dir", "."], 0);
    let (_, url) = env.upstream("lib");
    env.json(&project, &["add", &url], 0);
    let path = project.join(".tiger-state.json");
    let original = read(&path);
    let mut state: Value = serde_json::from_str(&original).unwrap();
    let record = state["refs"]["lib"].clone();
    state["refs"] = serde_json::json!({ "tiger.toml": record });
    std::fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let config = read(project.join("tiger.toml"));
    env.json(&project, &["sync"], 1);
    assert_eq!(read(project.join("tiger.toml")), config);
    assert!(project.join("lib/README.md").is_file());
    std::fs::write(&path, original).unwrap();
    assert_eq!(env.json(&project, &["sync"], 0)["refs"][0]["status"], "unchanged");
}

#[test]
fn initialization_refuses_output_files_and_symlinks_outside_the_project() {
    let env = Env::new();
    let root = env.project("app");
    let outside = env.root().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("user.txt"), "keep me").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("refs")).unwrap();
    for dir in ["refs", "refs/not-created"] {
        env.json(&root, &["init", "--dir", dir], 1);
        assert!(!root.join("tiger.toml").exists());
        assert!(!outside.join("not-created").exists());
        assert_eq!(read(outside.join("user.txt")), "keep me");
    }
    std::fs::write(root.join("occupied"), "user file").unwrap();
    env.json(&root, &["init", "--dir", "occupied"], 1);
    assert!(!root.join("tiger.toml").exists());
    assert_eq!(read(root.join("occupied")), "user file");
    env.json(&root, &["init", "--dir", "real-refs"], 0);
    assert!(root.join("tiger.toml").is_file());
    assert!(root.join("real-refs/TIGER.md").is_file());
    let config = read(root.join("tiger.toml"));
    std::fs::write(root.join("tiger.toml"), config.replace("real-refs", "refs")).unwrap();
    env.json(&root, &["sync"], 1);
    assert!(!env.store().exists());
    assert_eq!(read(outside.join("user.txt")), "keep me");
    std::fs::write(root.join("tiger.toml"), config).unwrap();
    env.json(&root, &["sync"], 0);
    assert!(root.join("real-refs/.tiger-state.json").is_file());
}

#[test]
fn deferred_addition_validates_locally_and_preserves_order_and_note_paragraphs() {
    let env = Env::new();
    let project = env.project("app");
    env.json(&project, &["init"], 0);
    let missing = format!("file://{}", env.root().join("missing").display());
    let before = read(project.join("tiger.toml"));
    env.json(&project, &["add", &missing, "--no-sync"], 1);
    assert_eq!(read(project.join("tiger.toml")), before);
    env.json(&project, &["add", &missing, "--no-validate"], 1);
    assert_eq!(read(project.join("tiger.toml")), before);
    let note = "A softly wrapped\nparagraph.\n\n- First item\n  - Nested item\n- Second item";
    let first = env.json(&project, &["add", &missing, "--as", "first", "--no-sync", "--no-validate", "--note", note], 0);
    assert_eq!(first["validation"], "deferred");
    assert!(first.get("tracks").is_none() || first["tracks"].is_null());
    env.json(&project, &["add", &missing, "--as", "second", "--ref", "v1", "--no-sync", "--no-validate"], 0);
    env.json(&project, &["add", &missing, "--as", "third", "--no-sync", "--no-validate"], 0);
    let text = read(project.join("tiger.toml"));
    assert!(text.contains("# A softly wrapped paragraph.\n#\n# - First item\n#   - Nested item\n# - Second item"), "{text}");
    assert!(text.contains("second = { url = "), "{text}");
    assert!(text.find("first =").unwrap() < text.find("second =").unwrap());
    assert!(text.find("second =").unwrap() < text.find("third =").unwrap());
    for args in [vec!["--as", "bad/id"], vec!["--include", "["], vec!["--history", "--include", "src/**"]] {
        let mut command = vec!["add", &missing, "--no-sync", "--no-validate"];
        command.extend(args);
        env.json(&project, &command, 1);
        assert_eq!(read(project.join("tiger.toml")), text);
    }
    let sync = env.json(&project, &["sync"], 2);
    assert_eq!(sync["summary"]["failed"], 3);
    let plain = env.root().join("new-project");
    std::fs::create_dir(&plain).unwrap();
    env.json(&plain, &["add", &missing, "--no-sync", "--no-validate", "--include", "["], 1);
    assert!(!plain.join("tiger.toml").exists());
    env.json(&plain, &["add", &missing, "--no-sync", "--no-validate"], 0);
    assert!(plain.join("tiger.toml").is_file());
}

#[test]
fn a_fallback_reused_as_a_shared_store_keeps_other_projects_cache_and_its_lock() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let env = Env::new();
    let (upstream, url) = env.upstream("lib");
    let sha = env.git(&upstream, &["rev-parse", "HEAD"]);
    let project = env.project("app");
    env.fallback_json(&project, &["add", &url, "--ref", &sha]);
    let source = project.join("tiger/.tiger-store");
    let other = env.project("other");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let out = cmd.current_dir(&other).env("TIGER_HOME", &source).args(["add", &url, "--ref", &sha, "--json"]).output().unwrap();
    assert!(out.status.success());
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(out["promotion"]["promoted_trees"], 1);
    assert_eq!(out["promotion"]["remaining_trees"], 1);
    assert!(!out["promotion"]["cleanup_warnings"].as_array().unwrap().is_empty());
    assert!(other.join("tiger/lib/README.md").is_file());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let mut child = cmd
        .current_dir(&project)
        .env("TIGER_HOME", &source)
        .args(["sync", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut finished = false;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !finished {
        let _ = child.kill();
    }
    let out = child.wait_with_output().unwrap();
    assert!(finished, "active store was locked twice through its fallback alias");
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["store"]["mode"], "shared");
    assert!(source.join("locks/store.lock").is_file());
}

#[test]
fn sync_reports_activity_before_and_during_a_lock_wait_without_changing_json() {
    use std::io::{BufRead, BufReader};
    use std::time::Duration;
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    env.json(&project, &["add", &url, "--no-sync"], 0);
    let lock = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(project.join("tiger/.tiger-lock")).unwrap();
    lock.lock().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let mut child = cmd
        .current_dir(&project)
        .args(["sync", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let _ = tx.send(line.unwrap());
        }
    });
    let first = rx.recv_timeout(Duration::from_secs(1));
    let mut waiting = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            waiting.push(line);
        }
    }
    std::fs::File::unlock(&lock).unwrap();
    let out = child.wait_with_output().unwrap();
    reader.join().unwrap();
    assert!(first.is_ok(), "no immediate progress before lock acquisition");
    assert!(
        waiting.iter().filter(|line| line.contains("waiting for another tiger process")).count() >= 2,
        "no recurring lock activity: {waiting:?}"
    );
    assert!(waiting.iter().all(|line| !line.contains("1/1 refs completed")), "premature completion: {waiting:?}");
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["refs"][0]["status"], "new");
    let quiet = env.tiger(&project, &["sync", "--json", "--quiet"]);
    assert!(quiet.status.success());
    assert!(quiet.stderr.is_empty(), "{}", String::from_utf8_lossy(&quiet.stderr));
    let json: Value = serde_json::from_slice(&quiet.stdout).unwrap();
    assert_eq!(json["refs"][0]["status"], "unchanged");
}

#[test]
fn shared_sync_promotes_selected_pins_and_complete_trees_without_rebuilding_copies() {
    use std::os::unix::fs::MetadataExt;
    let env = Env::new();
    let (upstream, url) = env.upstream("lib");
    let first = env.git(&upstream, &["rev-parse", "HEAD"]);
    let shared_project = env.project("existing-shared");
    env.json(&shared_project, &["add", &url, "--as", "older", "--ref", &first], 0);
    let second = env.commit(&upstream, &[("src/new.ts", "new source"), ("outside.txt", "unfiltered")]);
    let (other_upstream, other_url) = env.upstream("other");
    let other_sha = env.git(&other_upstream, &["rev-parse", "HEAD"]);
    let project = env.project("app");
    env.fallback_json(&project, &["add", &url, "--as", "lib", "--ref", &second, "--include", "src/**"]);
    env.fallback_json(&project, &["add", &other_url, "--as", "other", "--ref", &other_sha]);
    let before: Value = serde_json::from_str(&read(project.join("tiger/.tiger-state.json"))).unwrap();
    let inode = std::fs::metadata(project.join("tiger/lib/src/lib.ts")).unwrap().ino();
    std::fs::rename(&upstream, env.root().join("gone-lib")).unwrap();
    std::fs::rename(&other_upstream, env.root().join("gone-other")).unwrap();
    let out = env.json(&project, &["sync", "lib"], 0);
    assert_eq!(out["refs"][0]["status"], "unchanged");
    assert_eq!(out["promotion"]["promoted_trees"], 1);
    assert_eq!(out["promotion"]["remaining_trees"], 1);
    assert_eq!(out["registered"], true);
    assert_eq!(out["store"]["mode"], "shared");
    assert_eq!(find_dirs(&env.store().join("trees"), &second).len(), 1);
    let tree = find_dirs(&env.store().join("trees"), &second).pop().unwrap();
    assert_eq!(read(tree.join("outside.txt")), "unfiltered");
    assert!(!project.join("tiger/lib/outside.txt").exists());
    assert_eq!(std::fs::metadata(project.join("tiger/lib/src/lib.ts")).unwrap().ino(), inode);
    let after: Value = serde_json::from_str(&read(project.join("tiger/.tiger-state.json"))).unwrap();
    assert_eq!(after["refs"]["lib"]["synced_at"], before["refs"]["lib"]["synced_at"]);
    let fallback = project.join("tiger/.tiger-store");
    assert_eq!(find_dirs(&fallback.join("trees"), &other_sha).len(), 1);
    std::fs::write(fallback.join("my-notes.txt"), "keep me").unwrap();
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(out["promotion"]["promoted_trees"], 1);
    assert_eq!(out["promotion"]["remaining_trees"], 0);
    assert_eq!(out["promotion"]["remaining_repos"], 0);
    assert_eq!(read(fallback.join("my-notes.txt")), "keep me");
    assert!(fallback.join("locks/store.lock").is_file());
    let report = env.json(&project, &["show"], 0);
    assert_eq!(report["store"]["last_sync"]["mode"], "shared");
    assert_eq!(report["store"]["registered"], true);
    assert_eq!(report["store"]["fallback_trees"], 0);
    let existing = env.json(&shared_project, &["show"], 0);
    assert!(!existing["refs"][0]["commit"]["subject"].as_str().unwrap().is_empty());
    assert!(!report["refs"][0]["commit"]["subject"].as_str().unwrap().is_empty());
}

#[test]
fn failed_promotion_keeps_fallback_data_and_retries_while_other_refs_sync() {
    let env = Env::new();
    let (upstream, url) = env.upstream("lib");
    let sha = env.git(&upstream, &["rev-parse", "HEAD"]);
    let (_, good) = env.upstream("good");
    let project = env.project("app");
    env.fallback_json(&project, &["add", &url, "--as", "lib", "--ref", &sha]);
    env.json(&project, &["add", &good, "--as", "good", "--no-sync"], 0);
    let fallback = project.join("tiger/.tiger-store");
    let repo = find_dirs(&fallback.join("repos"), "lib.git").pop();
    // Locate the bare repo by HEAD if it wasn't found by its folder name.
    let repo = repo.unwrap_or_else(|| {
        fn bare(root: &Path) -> Option<PathBuf> {
            if root.join("HEAD").is_file() {
                return Some(root.to_path_buf());
            }
            for entry in std::fs::read_dir(root).ok()?.flatten() {
                if entry.path().is_dir()
                    && let Some(path) = bare(&entry.path())
                {
                    return Some(path);
                }
            }
            None
        }
        bare(&fallback.join("repos")).unwrap()
    });
    let saved = read(repo.join("HEAD"));
    std::fs::write(repo.join("HEAD"), "broken repository\n").unwrap();
    let out = env.json(&project, &["sync"], 2);
    assert_eq!(out["refs"][0]["status"], "unchanged");
    assert_eq!(out["refs"][1]["status"], "new");
    assert_eq!(out["promotion"]["failures"].as_array().unwrap().len(), 1);
    assert_eq!(out["promotion"]["remaining_trees"], 1);
    assert!(project.join("tiger/lib/README.md").is_file());
    assert!(project.join("tiger/good/README.md").is_file());
    assert!(env.ok(&project, &["sync"], 2).contains("· promotion failed 1"));
    std::fs::write(repo.join("HEAD"), saved).unwrap();
    let cached = find_dirs(&fallback.join("trees"), &sha).pop().unwrap();
    std::fs::remove_dir_all(cached).unwrap();
    std::fs::rename(upstream, env.root().join("gone")).unwrap();
    let out = env.json(&project, &["sync", "lib"], 0);
    assert_eq!(out["promotion"]["promoted_trees"], 1);
    assert_eq!(out["promotion"]["failures"], serde_json::json!([]));
    let tree = find_dirs(&env.store().join("trees"), &sha).pop().unwrap();
    assert_eq!(read(tree.join("README.md")), "# hello\n");
}

#[test]
fn verified_promotion_warns_on_cleanup_failure_and_retries_cleanup() {
    let env = Env::new();
    let (upstream, url) = env.upstream("lib");
    let sha = env.git(&upstream, &["rev-parse", "HEAD"]);
    let project = env.project("app");
    env.fallback_json(&project, &["add", &url, "--as", "lib", "--ref", &sha]);
    let fallback = project.join("tiger/.tiger-store");
    let tree = find_dirs(&fallback.join("trees"), &sha).pop().unwrap();
    let parent = tree.parent().unwrap();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o555)).unwrap();
    let output = env.tiger(&project, &["sync", "--json"]);
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["promotion"]["promoted_trees"], 1);
    assert!(result["promotion"]["failures"].as_array().unwrap().is_empty());
    assert!(!result["promotion"]["cleanup_warnings"].as_array().unwrap().is_empty());
    let shared = find_dirs(&env.store().join("trees"), &sha).pop().unwrap();
    assert_eq!(read(shared.join("README.md")), "# hello\n");
    std::fs::rename(upstream, env.root().join("gone")).unwrap();
    let result = env.json(&project, &["sync"], 0);
    assert_eq!(result["promotion"]["remaining_trees"], 0);
    assert!(result["promotion"]["cleanup_warnings"].as_array().unwrap().is_empty());
}

#[test]
fn introspection_reports_observed_stores_without_probing_or_creating_them() {
    let env = Env::new();
    let project = env.project("app");
    env.json(&project, &["init"], 0);
    assert!(!env.store().exists());
    let show = env.json(&project, &["show"], 0);
    assert!(show["store"]["last_sync"].is_null());
    assert_eq!(show["store"]["registered"], false);
    env.json(&project, &["store"], 0);
    assert!(!env.store().exists());
    assert!(!project.join("tiger/.tiger-lock").exists());
    assert!(!project.join("tiger/.tiger-state.json").exists());
    let (_, url) = env.upstream("lib");
    env.fallback_json(&project, &["add", &url]);
    let show = env.json(&project, &["show"], 0);
    assert_eq!(show["store"]["last_sync"]["mode"], "fallback");
    assert_eq!(show["store"]["fallback_trees"], 1);
    assert_eq!(show["store"]["registered"], false);
    assert!(!env.store().exists());
}

#[test]
fn shared_promotion_waits_for_fallback_sync_before_taking_project_locks() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    env.fallback_json(&project, &["add", &url]);
    env.json(&project, &["pin", "lib", "--no-sync"], 0);
    let lock = std::fs::OpenOptions::new().write(true).open(project.join("tiger/.tiger-lock")).unwrap();
    lock.lock().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let mut fallback = cmd
        .current_dir(&project)
        .env("TIGER_HOME", env.root().join("blocked-store"))
        .args(["sync", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let pipe = fallback.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            let _ = tx.send(line.unwrap());
        }
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut ready = false;
    while Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100))
            && line.contains("waiting for another tiger process")
        {
            ready = true;
            break;
        }
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let mut shared = cmd.current_dir(&project).args(["sync", "--json"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    std::fs::File::unlock(&lock).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut finished = false;
    while Instant::now() < deadline {
        if fallback.try_wait().unwrap().is_some() && shared.try_wait().unwrap().is_some() {
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !finished {
        let _ = fallback.kill();
        let _ = shared.kill();
    }
    let fallback_out = fallback.wait_with_output().unwrap();
    let shared_out = shared.wait_with_output().unwrap();
    reader.join().unwrap();
    assert!(ready, "fallback sync never reached the project lock");
    assert!(finished, "concurrent shared/fallback syncs deadlocked");
    assert!(fallback_out.status.success());
    assert!(shared_out.status.success(), "{}", String::from_utf8_lossy(&shared_out.stdout));
    let result: Value = serde_json::from_slice(&shared_out.stdout).unwrap();
    assert_eq!(result["promotion"]["promoted_trees"], 1);
    assert!(project.join("tiger/lib/README.md").is_file());
}

#[test]
fn the_documented_move_checks_destinations_and_preserves_state_and_user_files() {
    let env = Env::new();
    let root = env.project("app");
    let project = root.join("__references__");
    std::fs::create_dir(&project).unwrap();
    env.json(&project, &["init"], 0);
    let (upstream, url) = env.upstream("lib");
    let sha = env.git(&upstream, &["rev-parse", "HEAD"]);
    env.fallback_json(&project, &["add", &url, "--ref", &sha, "--note", "Keep this note."]);
    let config = format!("# Reminder: dir = \"tiger\"\n{}", read(project.join("tiger.toml")));
    std::fs::write(project.join("tiger.toml"), &config).unwrap();
    let old_state: Value = serde_json::from_str(&read(project.join("tiger/.tiger-state.json"))).unwrap();
    std::fs::write(project.join("tiger/user-notes.txt"), "stay here").unwrap();
    std::fs::create_dir(project.join("lib")).unwrap();
    std::fs::write(project.join("lib/user.txt"), "existing destination").unwrap();
    let guide = read(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guide/moving-copies-beside-config.md"));
    let script = guide.split("<!-- relocation-script -->\n\n```sh\n").nth(1).unwrap().split("\n```").next().unwrap();
    let run = || {
        let mut cmd = Command::new("sh");
        hermetic(&mut cmd, env.root());
        cmd.current_dir(&project).args(["-c", script]).output().unwrap()
    };
    assert!(!run().status.success());
    assert_eq!(read(project.join("tiger.toml")), config);
    assert!(project.join("tiger/lib/README.md").is_file());
    assert_eq!(read(project.join("lib/user.txt")), "existing destination");
    std::fs::rename(project.join("lib"), project.join("user-folder")).unwrap();
    let output = run();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(read(project.join("tiger.toml")), config.replacen("\ndir = \"tiger\"\n", "\ndir = \".\"\n", 1));
    assert_eq!(read(project.join("tiger/user-notes.txt")), "stay here");
    assert_eq!(read(project.join("lib/README.md")), "# hello\n");
    assert!(project.join(".tiger-store").is_dir());
    std::fs::rename(upstream, env.root().join("gone")).unwrap();
    let sync = env.json(&project, &["sync"], 0);
    assert_eq!(sync["refs"][0]["status"], "unchanged");
    assert_eq!(sync["promotion"]["promoted_trees"], 1);
    let new_state: Value = serde_json::from_str(&read(project.join(".tiger-state.json"))).unwrap();
    assert_eq!(new_state["refs"]["lib"]["synced_at"], old_state["refs"]["lib"]["synced_at"]);
    assert!(read(project.join("TIGER.md")).contains("](tiger.toml)"));
    assert_eq!(read(project.join("user-folder/user.txt")), "existing destination");
}

#[test]
fn local_git_exclusion_works_in_a_worktree_without_editing_tracked_instructions() {
    let env = Env::new();
    let (repo, _) = env.upstream("root");
    let worktree = env.root().join("worktree");
    env.git(&repo, &["worktree", "add", "--quiet", "--detach", worktree.to_str().unwrap()]);
    assert!(worktree.join(".git").is_file());
    std::fs::write(worktree.join("AGENTS.md"), "User instructions.\n").unwrap();
    let references = worktree.join("refs");
    std::fs::create_dir(&references).unwrap();
    let exclude = env.git(&worktree, &["rev-parse", "--git-path", "info/exclude"]);
    let exclude = worktree.join(exclude);
    let mut contents = read(&exclude);
    contents.push_str("\n/refs/\n");
    std::fs::write(exclude, contents).unwrap();
    env.json(&references, &["init", "--dir", "."], 0);
    assert_eq!(env.git(&worktree, &["check-ignore", "refs/tiger.toml"]), "refs/tiger.toml");
    assert!(!worktree.join(".gitignore").exists());
    assert_eq!(read(worktree.join("AGENTS.md")), "User instructions.\n");
}

#[test]
fn init_writes_a_commented_config_the_gitignore_line_and_the_pointer() {
    let env = Env::new();
    let project = env.project("app");
    std::fs::write(project.join(".gitignore"), "node_modules/").unwrap();
    env.ok(&project, &["init"], 0);
    let config = read(project.join("tiger.toml"));
    assert!(config.starts_with("# ── tiger"), "{config}");
    assert!(config.contains("dir = \"tiger\"\n\n[refs]\n"), "{config}");
    assert_eq!(read(project.join(".gitignore")), "node_modules/\n\n# tiger: read-only copies of other repos (see tiger.toml)\n/tiger/\n");
    assert!(read(project.join("tiger/TIGER.md")).contains("[`tiger.toml`](../tiger.toml)"));
    // Re-running changes nothing.
    env.ok(&project, &["init"], 0);
    assert_eq!(read(project.join(".gitignore")).matches("/tiger/").count(), 1);
    // Projects decide: --no-gitignore leaves .gitignore alone.
    let other = env.project("other");
    env.ok(&other, &["init", "--dir", "refs", "--no-gitignore"], 0);
    assert!(!other.join(".gitignore").exists());
    assert!(read(other.join("tiger.toml")).contains("dir = \"refs\""));
}

#[test]
fn a_url_alone_follows_the_default_branch_with_read_only_copies() {
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    let out = env.ok(&project, &["add", &url, "--note", "The library we wrap."], 0);
    assert!(out.contains("added lib, follows trunk, the default branch"), "{out}");
    assert!(read(project.join("tiger.toml")).contains(&format!("\n# The library we wrap.\nlib = \"{url}\"\n")));
    assert_eq!(read(project.join("tiger/lib/src/lib.ts")), "export const a = 1\n");
    let mode = std::fs::metadata(project.join("tiger/lib/README.md")).unwrap().permissions().mode();
    assert_eq!(mode & 0o222, 0, "copies are read-only");
    assert!(std::fs::write(project.join("tiger/lib/README.md"), "edit").is_err());
    // Adding it again is a no-op.
    let again = env.ok(&project, &["add", &url], 0);
    assert!(again.contains("lib is already a ref"), "{again}");
    assert_eq!(read(project.join("tiger.toml")).matches("lib = ").count(), 1);
}

#[test]
fn sync_fetches_only_what_changed() {
    let env = Env::new();
    let (a_dir, a) = env.upstream("a");
    let (_, b) = env.upstream("b");
    let project = env.project("app");
    env.ok(&project, &["add", &a, "--no-sync"], 0);
    env.ok(&project, &["add", &b, "--no-sync"], 0);
    let first = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&first), [pair("a", "new"), pair("b", "new")]);
    assert_eq!(first["summary"]["synced"], 2);

    let unchanged = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&unchanged), [pair("a", "unchanged"), pair("b", "unchanged")]);

    let before = env.git(&a_dir, &["rev-parse", "HEAD"]);
    let after = env.commit(&a_dir, &[("src/lib.ts", "export const a = 2\n")]);
    let status = env.json(&project, &["status"], 0);
    assert_eq!(status["refs"][0]["verdict"], "behind");
    assert_eq!(status["refs"][1]["verdict"], "up-to-date");

    let changed = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&changed), [pair("a", "changed"), pair("b", "unchanged")]);
    assert_eq!(changed["refs"][0]["from"], before.as_str());
    assert_eq!(changed["refs"][0]["to"], after.as_str());
    assert_eq!(changed["refs"][0]["path"], "tiger/a");
    assert_eq!(read(project.join("tiger/a/src/lib.ts")), "export const a = 2\n");

    // --force rebuilds even when nothing changed.
    let forced = env.json(&project, &["sync", "--force", "b"], 0);
    assert_eq!(statuses(&forced), [pair("b", "changed")]);
}

#[test]
fn failures_keep_the_existing_copy_and_the_rest_still_sync() {
    let env = Env::new();
    let (a_dir, a) = env.upstream("a");
    let (b_dir, b) = env.upstream("b");
    let project = env.project("app");
    env.ok(&project, &["add", &a], 0);
    env.ok(&project, &["add", &b], 0);
    std::fs::rename(&a_dir, env.root().join("moved")).unwrap();
    env.commit(&b_dir, &[("new.txt", "new")]);
    let out = env.json(&project, &["sync"], 2);
    assert_eq!(statuses(&out), [pair("a", "failed"), pair("b", "changed")]);
    let error = out["refs"][0]["error"].as_str().unwrap();
    assert!(error.starts_with("git: ") && error.ends_with("kept existing copy"), "{error}");
    assert!(project.join("tiger/a/README.md").exists(), "the old copy stays");
    assert!(project.join("tiger/b/new.txt").exists());
    let text = env.ok(&project, &["sync"], 2);
    assert!(text.contains("! failed     a"), "{text}");
}

#[test]
fn tiger_only_touches_what_it_created() {
    let env = Env::new();
    let (_, a) = env.upstream("a");
    let (_, b) = env.upstream("b");
    let project = env.project("app");
    env.ok(&project, &["add", &a], 0);
    std::fs::write(project.join("tiger/diagram.png"), "png").unwrap();
    std::fs::create_dir_all(project.join("tiger/notes")).unwrap();
    std::fs::write(project.join("tiger/notes/x.md"), "mine").unwrap();
    // A folder tiger didn't create blocks a new ref of that name instead of being replaced.
    std::fs::create_dir_all(project.join("tiger/b")).unwrap();
    std::fs::write(project.join("tiger/b/keep.txt"), "mine").unwrap();
    let out = env.json(&project, &["add", &b], 2);
    let error = out["sync"]["projects"][0]["refs"][0]["error"].as_str().unwrap();
    assert!(error.contains("already exists and tiger didn't create it"), "{error}");
    env.ok(&project, &["rm", "a"], 0);
    env.ok(&project, &["sync", "--force"], 2);
    assert_eq!(read(project.join("tiger/diagram.png")), "png");
    assert_eq!(read(project.join("tiger/notes/x.md")), "mine");
    assert_eq!(read(project.join("tiger/b/keep.txt")), "mine");
    assert!(!project.join("tiger/a").exists());
}

#[test]
fn removing_an_entry_from_the_config_deletes_its_copy_on_the_next_sync() {
    let env = Env::new();
    let (_, a) = env.upstream("a");
    let (_, b) = env.upstream("b");
    let project = env.project("app");
    env.ok(&project, &["add", &a, "--note", "Keep me."], 0);
    env.ok(&project, &["add", &b, "--note", "Drop me."], 0);
    // Edit by hand, as an agent might.
    let config = read(project.join("tiger.toml")).replace(&format!("\n# Drop me.\nb = \"{b}\"\n"), "");
    std::fs::write(project.join("tiger.toml"), config).unwrap();
    assert_eq!(env.json(&project, &["status", "--offline"], 0)["refs"][1]["verdict"], "removed");
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&out), [pair("a", "unchanged"), pair("b", "removed")]);
    assert!(!project.join("tiger/b").exists());
    // `tiger rm` removes the entry with its note.
    env.ok(&project, &["rm", "a"], 0);
    let config = read(project.join("tiger.toml"));
    assert!(!config.contains("Keep me."), "{config}");
    assert!(!project.join("tiger/a").exists());
}

#[test]
fn pin_and_unpin_switch_between_a_fixed_commit_and_the_default_branch() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let project = env.project("app");
    env.ok(&project, &["add", &url, "--note", "Pinned soon."], 0);
    let first = env.git(&dir, &["rev-parse", "HEAD"]);
    env.git(&dir, &["tag", "v1"]);
    let out = env.ok(&project, &["pin", "lib"], 0);
    assert!(out.contains(&format!("pinned lib to {}", &first[..7])), "{out}");
    let config = read(project.join("tiger.toml"));
    assert!(config.contains(&format!("# Pinned soon.\nlib = {{ url = \"{url}\", ref = \"{first}\" }}")), "{config}");

    env.commit(&dir, &[("later.txt", "later")]);
    assert_eq!(env.json(&project, &["sync"], 0)["refs"][0]["status"], "unchanged");
    assert!(!project.join("tiger/lib/later.txt").exists());

    env.ok(&project, &["pin", "lib", "v1"], 0);
    assert!(read(project.join("tiger.toml")).contains("ref = \"v1\""));
    let err = env.ok(&project, &["pin", "lib", "trunk"], 1);
    assert!(err.contains("'trunk' is a branch"), "{err}");

    env.ok(&project, &["unpin", "lib"], 0);
    assert!(read(project.join("tiger.toml")).contains(&format!("# Pinned soon.\nlib = \"{url}\"\n")));
    assert!(project.join("tiger/lib/later.txt").exists());
}

#[test]
fn pinned_refs_that_are_synced_need_no_network() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let sha = env.git(&dir, &["rev-parse", "HEAD"]);
    env.git(&dir, &["tag", "-a", "v1", "-m", "annotated"]);
    let project = env.project("app");
    env.ok(&project, &["add", &url, "--as", "tagged", "--ref", "v1"], 0);
    env.ok(&project, &["add", &url, "--as", "commit", "--ref", &sha], 0);
    let state: Value = serde_json::from_str(&read(project.join("tiger/.tiger-state.json"))).unwrap();
    assert_eq!(state["refs"]["tagged"]["commit"], sha.as_str(), "annotated tags resolve to their commit");
    std::fs::remove_dir_all(&dir).unwrap();
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&out), [pair("tagged", "unchanged"), pair("commit", "unchanged")]);
}

#[test]
fn a_name_that_is_both_a_branch_and_a_tag_is_an_error_not_a_guess() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    env.git(&dir, &["tag", "v1"]);
    env.git(&dir, &["branch", "v1"]);
    let project = env.project("app");
    let err = env.ok(&project, &["add", &url, "--ref", "v1"], 1);
    assert!(err.contains("'v1' is both a branch and a tag") && err.contains("refs/tags/v1"), "{err}");
    assert!(!read(project.join("tiger.toml")).contains(&url), "nothing is written");
    env.ok(&project, &["add", &url, "--ref", "refs/tags/v1"], 0);
}

#[test]
fn include_and_exclude_filter_the_copy() {
    let env = Env::new();
    let (dir, url) = env.upstream("big");
    env.commit(&dir, &[("src/a.snap", "snap"), ("docs/guide.md", "guide"), ("assets/huge.bin", "bin")]);
    let project = env.project("app");
    env.ok(&project, &["add", &url, "--include", "src/**", "docs/**", "--exclude", "**/*.snap"], 0);
    let copy = project.join("tiger/big");
    assert!(copy.join("src/lib.ts").exists());
    assert!(copy.join("docs/guide.md").exists());
    assert!(!copy.join("src/a.snap").exists());
    assert!(!copy.join("assets").exists());
    assert!(!copy.join("README.md").exists());
    // Changing filters rebuilds the copy even though the commit didn't change.
    let config = read(project.join("tiger.toml")).replace(", exclude = [\"**/*.snap\"]", "");
    std::fs::write(project.join("tiger.toml"), config).unwrap();
    assert_eq!(env.json(&project, &["status", "--offline"], 0)["refs"][0]["verdict"], "config-changed");
    env.ok(&project, &["sync"], 0);
    assert!(copy.join("src/a.snap").exists());
}

#[test]
fn projects_share_one_tree_in_the_store() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let sha = env.git(&dir, &["rev-parse", "HEAD"]);
    let one = env.project("one");
    let two = env.project("two");
    env.ok(&one, &["add", &url], 0);
    env.ok(&two, &["add", &url, "--as", "library"], 0);
    let tree = find_dirs(&env.store().join("trees"), &sha);
    assert_eq!(tree.len(), 1);
    assert_eq!(std::fs::read_dir(tree[0].parent().unwrap()).unwrap().count(), 1, "one tree for both projects");
    let state: Value = serde_json::from_str(&read(two.join("tiger/.tiger-state.json"))).unwrap();
    if cfg!(target_os = "macos") {
        assert_eq!(state["refs"]["library"]["copy_mode"], "clone", "APFS clones copy-on-write");
    } else {
        assert!(["clone", "copy"].contains(&state["refs"]["library"]["copy_mode"].as_str().unwrap()));
    }
    let registry: Value = serde_json::from_str(&read(env.store().join("registry.json"))).unwrap();
    assert_eq!(registry["projects"].as_object().unwrap().len(), 2);
}

#[test]
fn everywhere_updates_every_project_once_and_honors_propagate() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let one = env.project("one");
    let two = env.project("two");
    let three = env.project("three");
    env.ok(&one, &["add", &url], 0);
    env.ok(&two, &["add", &url], 0);
    env.ok(&three, &["add", &url, "--no-propagate"], 0);
    let latest = env.commit(&dir, &[("new.txt", "new")]);

    let out = env.json(&one, &["sync", "--everywhere"], 0);
    let projects = out["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 3);
    assert!(read(two.join("tiger/lib/new.txt")) == "new", "another project was updated");
    assert!(!three.join("tiger/lib/new.txt").exists(), "propagate = false is left alone");
    let three_report = projects.iter().find(|p| p["project"].as_str().unwrap().ends_with("three")).unwrap();
    assert_eq!(three_report["refs"].as_array().unwrap().len(), 0);
    let state: Value = serde_json::from_str(&read(two.join("tiger/.tiger-state.json"))).unwrap();
    assert_eq!(state["refs"]["lib"]["commit"], latest.as_str());
    // Its own sync still updates it.
    env.ok(&three, &["sync"], 0);
    assert!(three.join("tiger/lib/new.txt").exists());

    // The scheduled job runs from no project at all, and forgets deleted projects.
    std::fs::remove_file(two.join("tiger.toml")).unwrap();
    env.commit(&dir, &[("newer.txt", "newer")]);
    let text = env.ok(env.root(), &["sync", "--everywhere"], 0);
    assert!(text.contains("forgot"), "{text}");
    assert!(one.join("tiger/lib/newer.txt").exists());
    let registry: Value = serde_json::from_str(&read(env.store().join("registry.json"))).unwrap();
    assert_eq!(registry["projects"].as_object().unwrap().len(), 2);
}

#[test]
fn history_refs_are_real_clones_where_git_log_works() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    env.commit(&dir, &[("second.txt", "2")]);
    let project = env.project("app");
    env.ok(&project, &["add", &url, "--history"], 0);
    let copy = project.join("tiger/lib");
    assert_eq!(env.git(&copy, &["rev-list", "--count", "HEAD"]), "2");
    let latest = env.commit(&dir, &[("third.txt", "3")]);
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(out["refs"][0]["status"], "changed");
    assert_eq!(env.git(&copy, &["rev-parse", "HEAD"]), latest);
    assert_eq!(env.git(&copy, &["status", "--porcelain"]), "", "read-only files don't dirty the clone");
    assert!(std::fs::write(copy.join("third.txt"), "edit").is_err());
}

#[test]
fn invalid_configs_report_every_problem() {
    let env = Env::new();
    let project = env.project("app");
    std::fs::write(project.join("tiger.toml"), "dir = \".refs\"\n[refs.a]\nurl = \"x\"\ninlcude = [\"src\"]\n").unwrap();
    let out = env.json(&project, &["sync"], 1);
    let context = out["error"]["context"].as_array().unwrap();
    assert!(context.iter().any(|c| c.as_str().unwrap().contains("must not be hidden")), "{out}");
    assert!(context.iter().any(|c| c.as_str().unwrap().contains("did you mean 'include'?")), "{out}");
    let text = env.ok(&project, &["status"], 1);
    assert!(text.starts_with("tiger: invalid tiger.toml (2 problems)"), "{text}");
}

#[test]
fn an_unwritable_store_falls_back_to_one_inside_the_refs_folder() {
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    let locked = env.root().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let out = cmd.current_dir(&project).env("TIGER_HOME", locked.join("store")).args(["add", &url]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("using tiger/.tiger-store instead"), "{stderr}");
    assert!(project.join("tiger/.tiger-store/trees").is_dir());
    assert!(project.join("tiger/lib/README.md").exists());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let everywhere = cmd.current_dir(&project).env("TIGER_HOME", locked.join("store")).args(["sync", "--everywhere"]).output().unwrap();
    assert_eq!(everywhere.status.code(), Some(1));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn gc_prunes_what_no_project_uses() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let (_, other) = env.upstream("other");
    let project = env.project("app");
    env.ok(&project, &["add", &url], 0);
    env.ok(&project, &["add", &other], 0);
    let old = env.git(&dir, &["rev-parse", "HEAD"]);
    let new = env.commit(&dir, &[("x.txt", "x")]);
    env.ok(&project, &["sync"], 0);
    env.ok(&project, &["rm", "other"], 0);
    let out = env.json(&project, &["gc"], 0);
    assert_eq!(out["removed_trees"], 2, "the old commit of lib, and other");
    assert_eq!(out["removed_repos"], 1);
    assert_eq!(find_dirs(&env.store().join("trees"), &new).len(), 1);
    assert!(find_dirs(&env.store().join("trees"), &old).is_empty());
    // The copy still syncs from the pruned store.
    env.ok(&project, &["sync", "--force"], 0);
}

#[test]
fn global_settings_live_in_one_file_and_schedule_installs_the_job() {
    let env = Env::new();
    let here = env.root();
    env.ok(here, &["config", "set", "schedule", "6h"], 0);
    env.ok(here, &["config", "set", "dir", "refs"], 0);
    let settings = read(here.join("xdg/tiger/config.toml"));
    assert!(settings.contains("schedule = \"6h\"") && settings.contains("dir = \"refs\""), "{settings}");
    let job = if cfg!(target_os = "macos") { here.join("jobs/tiger.sync.plist") } else { here.join("jobs/tiger-sync.timer") };
    let text = read(&job);
    assert!(text.contains("21600"), "{text}");
    let listed = env.json(here, &["config"], 0);
    assert_eq!(listed["schedule_job"]["installed"], true);
    assert_eq!(env.ok(here, &["config", "get", "schedule"], 0).trim(), "6h");
    env.ok(here, &["config", "set", "schedule", "1h"], 0);
    assert!(read(&job).contains("3600"));
    let err = env.ok(here, &["config", "set", "schedule", "5m"], 1);
    assert!(err.contains("too frequent"), "{err}");
    let err = env.ok(here, &["config", "set", "scheduel", "1h"], 1);
    assert!(err.contains("did you mean 'schedule'?"), "{err}");
    env.ok(here, &["config", "set", "schedule", "off"], 0);
    assert!(!job.exists());
    // `dir` sets the folder for new projects.
    let project = env.project("app");
    env.ok(&project, &["init"], 0);
    assert!(read(project.join("tiger.toml")).contains("dir = \"refs\""));
    assert!(project.join("refs/TIGER.md").exists());
}

#[test]
fn release_notes_go_when_releases_is_turned_off() {
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    env.ok(&project, &["add", &url], 0);
    // As left behind by an earlier `releases = true`.
    std::fs::write(project.join("tiger/lib.releases.md"), "# notes").unwrap();
    std::fs::write(project.join("tiger/other.releases.md"), "mine").unwrap();
    env.ok(&project, &["sync"], 0);
    assert!(!project.join("tiger/lib.releases.md").exists());
    assert!(project.join("tiger/other.releases.md").exists(), "not a ref's notes");
}

#[test]
fn global_git_config_applies_and_a_projects_local_config_does_not() {
    let env = Env::new();
    let (dir, _) = env.upstream("private");
    let alias = "https://git.example.invalid/me/private";
    let project = env.project("app");
    // A rewrite in the project's own git config would apply only where git runs inside the project.
    env.git(&project, &["config", "--local", &format!("url.file://{}.insteadOf", dir.display()), alias]);
    let err = env.ok(&project, &["add", alias, "--as", "private"], 1);
    assert!(err.contains("can't reach"), "{err}");
    // The same rewrite in the global config applies everywhere: checks, fetches, the scheduled job.
    let global = env.root().join("gitconfig");
    std::fs::write(&global, format!("[url \"file://{}\"]\n\tinsteadOf = {alias}\n", dir.display())).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tiger"));
    hermetic(&mut cmd, env.root());
    let out = cmd.current_dir(&project).env("GIT_CONFIG_GLOBAL", &global).args(["add", alias, "--as", "private"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(project.join("tiger/private/README.md").exists());
    assert!(read(project.join("tiger.toml")).contains(alias), "the config keeps the URL as written");
}

#[test]
fn show_and_store_say_what_each_copy_holds_without_the_network() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let first = env.git(&dir, &["rev-parse", "HEAD"]);
    let one = env.project("one");
    let two = env.project("two");
    env.ok(&one, &["add", &url], 0);
    env.ok(&two, &["add", &url, "--as", "library"], 0);
    env.ok(&one, &["add", &url, "--as", "later", "--no-sync"], 0);
    let latest = env.commit(&dir, &[("src/lib.ts", "export const a = 2\n")]);
    env.ok(&two, &["sync"], 0);
    std::fs::remove_dir_all(&dir).unwrap();

    let shown = env.json(&one, &["show"], 0);
    let lib = &shown["refs"][0];
    assert_eq!(lib["commit"]["sha"], first.as_str());
    assert_eq!(lib["commit"]["subject"], "change");
    assert_eq!(lib["commit"]["date"].as_str().unwrap().len(), 10);
    assert_eq!(lib["files"], 2);
    assert_eq!(lib["elsewhere"][0]["id"], "library");
    assert_eq!(lib["elsewhere"][0]["commit"], latest.as_str(), "the other project moved on");
    assert_eq!(shown["refs"][1]["synced"], false, "a ref that never synced says so");
    let text = env.ok(&one, &["show", "lib"], 0);
    assert!(text.contains(&format!("commit    {}", &first[..7])) && text.contains("(default branch)"), "{text}");

    let store = env.json(&one, &["store"], 0);
    let trees = store["repos"][0]["trees"].as_array().unwrap();
    assert_eq!(trees.len(), 2, "one tree per commit in use");
    let used = |sha: &str| trees.iter().find(|t| t["commit"]["sha"] == sha).unwrap()["used_by"][0]["id"].clone();
    assert_eq!(used(&first), "lib");
    assert_eq!(used(&latest), "library");
    env.ok(&one, &["rm", "lib"], 0);
    let store = env.json(&one, &["store"], 0);
    let unused = store["repos"][0]["trees"].as_array().unwrap().iter().filter(|t| t["used_by"].as_array().unwrap().is_empty()).count();
    assert_eq!(unused, 1, "gc's candidates are visible before running it");
}

#[test]
fn the_rebuild_recipe_moves_copies_without_losing_the_old_ones_first() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let (_, other) = env.upstream("other");
    let sha = env.git(&dir, &["rev-parse", "HEAD"]);
    let project = env.project("app");
    let refs = project.join("__references__");
    std::fs::create_dir(&refs).unwrap();
    env.json(&refs, &["init", "--dir", "tiger", "--no-gitignore"], 0);
    env.json(&refs, &["add", &url], 0);
    env.json(&refs, &["add", &url, "--as", "pinned", "--ref", &sha], 0);
    env.json(&refs, &["add", &other], 0);
    std::fs::write(refs.join("tiger/notes.md"), "mine").unwrap();
    // A folder that isn't tiger's, where one copy would land.
    std::fs::create_dir(refs.join("other")).unwrap();
    std::fs::write(refs.join("other/keep.txt"), "mine").unwrap();

    // Step 3: change dir and sync; the old copies stay until step 4.
    let config = read(refs.join("tiger.toml")).replace("dir = \"tiger\"", "dir = \".\"");
    std::fs::write(refs.join("tiger.toml"), config).unwrap();
    let out = env.json(&refs, &["sync"], 2);
    assert_eq!(statuses(&out), [pair("lib", "new"), pair("pinned", "new"), pair("other", "failed")]);
    assert!(refs.join("lib/README.md").is_file() && refs.join("pinned/README.md").is_file());
    assert_eq!(read(refs.join("other/keep.txt")), "mine", "a folder tiger didn't create is never overwritten");
    assert!(refs.join("tiger/lib/README.md").is_file(), "old copies remain until the user deletes them");
    assert_eq!(read(refs.join("tiger/notes.md")), "mine");

    // Step 4: keep your files, delete the old folder, and everything still syncs.
    std::fs::rename(refs.join("tiger/notes.md"), refs.join("notes.md")).unwrap();
    env.ok(&refs, &["rm", "other"], 0);
    assert_eq!(read(refs.join("other/keep.txt")), "mine", "rm deletes only copies tiger made");
    std::fs::remove_dir_all(refs.join("tiger")).unwrap();
    let out = env.json(&refs, &["sync"], 0);
    assert_eq!(statuses(&out), [pair("lib", "unchanged"), pair("pinned", "unchanged")]);
    assert_eq!(read(refs.join("notes.md")), "mine");
}

#[test]
fn annotated_tags_resolve_to_the_commit_they_point_at() {
    let env = Env::new();
    let (dir, url) = env.upstream("lib");
    let commit = env.git(&dir, &["rev-parse", "HEAD"]);
    env.git(&dir, &["tag", "-a", "v1", "-m", "annotated"]);
    let tag = env.git(&dir, &["rev-parse", "v1"]);
    assert_ne!(tag, commit, "an annotated tag is its own object");
    let project = env.project("app");
    env.json(&project, &["add", &url, "--as", "first", "--ref", "v1"], 0);
    // The store now holds the tag object, which is when a tag's own ID used to leak through.
    env.json(&project, &["add", &url, "--as", "second", "--ref", "v1"], 0);
    let shown = env.json(&project, &["show"], 0);
    for r in shown["refs"].as_array().unwrap() {
        assert_eq!(r["commit"]["sha"], commit.as_str(), "{} holds the tagged commit", r["id"]);
    }
    let status = env.json(&project, &["status"], 0);
    for r in status["refs"].as_array().unwrap() {
        assert_eq!(r["verdict"], "pinned", "{r}");
        assert_eq!(r["upstream"], commit.as_str());
    }
    // A state file written by the bug (the tag's own ID) is corrected by the next sync.
    let state_path = project.join("tiger/.tiger-state.json");
    let mut state: Value = serde_json::from_str(&read(&state_path)).unwrap();
    state["refs"]["second"]["commit"] = Value::from(tag.as_str());
    std::fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let out = env.json(&project, &["sync"], 0);
    assert_eq!(statuses(&out), [pair("first", "unchanged"), pair("second", "changed")]);
    assert_eq!(out["refs"][1]["to"], commit.as_str());
}

#[test]
fn show_prints_notes_status_takes_ids_and_rm_says_where_the_data_went() {
    let env = Env::new();
    let (_, a) = env.upstream("a");
    let (_, b) = env.upstream("b");
    let project = env.project("app");
    env.json(&project, &["add", &a, "--note", "Why a matters.\n\nSecond paragraph."], 0);
    env.json(&project, &["add", &b], 0);
    let shown = env.json(&project, &["show"], 0);
    assert_eq!(shown["refs"][0]["note"], "Why a matters.\n\nSecond paragraph.");
    assert!(shown["refs"][1].get("note").is_none(), "a ref without a note has none");
    let text = env.ok(&project, &["show", "a"], 0);
    assert!(text.contains("  note      Why a matters."), "{text}");

    let status = env.json(&project, &["status", "--offline", "b"], 0);
    let ids: Vec<_> = status["refs"].as_array().unwrap().iter().map(|r| r["id"].clone()).collect();
    assert_eq!(ids, ["b"], "only the named ref, and the others aren't reported as removed");
    let err = env.ok(&project, &["status", "c"], 1);
    assert!(err.contains("unknown ref 'c'"), "{err}");

    let out = env.ok(&project, &["rm", "a"], 0);
    assert!(out.contains("`tiger gc`"), "{out}");
}

#[test]
fn add_creates_the_config_when_there_is_none() {
    let env = Env::new();
    let (_, url) = env.upstream("lib");
    let project = env.project("app");
    std::fs::create_dir_all(project.join("src/deep")).unwrap();
    env.ok(&project.join("src/deep"), &["add", &url], 0);
    assert!(project.join("tiger.toml").is_file(), "at the git root");
    assert!(project.join("tiger/lib/README.md").is_file());
    // Commands work from any subfolder.
    env.ok(&project.join("src/deep"), &["status"], 0);
}
