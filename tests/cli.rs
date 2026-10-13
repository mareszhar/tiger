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
    let config = read(project.join("tiger.toml")).replace("exclude = [\"**/*.snap\"]\n", "");
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
