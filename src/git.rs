//! Running git. Every call is non-interactive: no password prompts, no SSH
//! questions, no LFS downloads.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A remote that stops answering fails the ref instead of stalling the sync.
/// `ls-remote` normally answers in a second or two; one that stalls is retried.
pub const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(15);
pub const TIMED_OUT: &str = "no answer from the remote";
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A failed git command, reduced to the line that explains it.
#[derive(Debug, Clone)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn command() -> Command {
    let mut cmd = Command::new("git");
    // Abort transfers that stall below 1 KB/s for 30 seconds.
    cmd.args(["-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=30"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .stdin(Stdio::null());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o ServerAliveCountMax=3");
    }
    cmd
}

/// Runs git with `args` (in `dir`, when given) and returns its stdout.
pub fn run(dir: Option<&Path>, args: &[&str]) -> Result<String, GitError> {
    run_with(command(), dir, args)
}

pub fn run_with(mut cmd: Command, dir: Option<&Path>, args: &[&str]) -> Result<String, GitError> {
    // Outside any repository, only global and system config apply, so a ref
    // resolves the same from every project and from the scheduled job.
    cmd.current_dir(dir.unwrap_or(Path::new("/")));
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let timeout = match args.first().copied() {
        Some("ls-remote") => Some(LS_REMOTE_TIMEOUT),
        Some("fetch" | "clone") => Some(TRANSFER_TIMEOUT),
        _ => None,
    };
    // Its own process group, so a timeout also stops helpers like git-remote-https.
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(GitError("git is not installed or not on PATH".into()));
        }
        Err(err) => return Err(GitError(format!("could not run git: {err}"))),
    };
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if timeout.is_some_and(|t| started.elapsed() > t) => {
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => return Err(GitError(format!("could not run git: {err}"))),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    match status {
        None => Err(GitError(format!("{TIMED_OUT} in {}s (git {})", timeout.unwrap_or_default().as_secs(), args[0]))),
        Some(status) if status.success() => Ok(String::from_utf8_lossy(&stdout).into_owned()),
        Some(_) => Err(GitError(summarize(&String::from_utf8_lossy(&stderr), args))),
    }
}

/// Picks the most telling line of git's stderr: the first `fatal:`/`error:`
/// line, preferring the remote's own message when there is one.
fn summarize(stderr: &str, args: &[&str]) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let pick = lines
        .iter()
        .find(|l| l.contains("Permission denied") || l.contains("Repository not found") || l.contains("not found"))
        .or_else(|| lines.iter().find(|l| l.starts_with("fatal:") || l.starts_with("error:")))
        .or_else(|| lines.last());
    match pick {
        Some(line) => {
            let line = line.trim_start_matches("fatal: ").trim_start_matches("error: ");
            format!("git: {line}")
        }
        None => format!("git {} failed", args.first().copied().unwrap_or("")),
    }
}

/// Whether the object exists in the repository (`git cat-file -e`).
pub fn has_commit(git_dir: &Path, sha: &str) -> bool {
    let target = format!("{sha}^{{commit}}");
    command()
        .args(["--git-dir"])
        .arg(git_dir)
        .args(["cat-file", "-e", &target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn is_full_sha(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_auth_failures() {
        let stderr = "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n";
        assert_eq!(summarize(stderr, &["ls-remote"]), "git: git@github.com: Permission denied (publickey).");
        assert_eq!(summarize("fatal: bad thing\n", &["fetch"]), "git: bad thing");
    }
}
