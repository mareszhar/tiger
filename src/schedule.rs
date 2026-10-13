//! The background job behind the `schedule` setting: it runs
//! `tiger sync --everywhere --quiet` at an interval. A launchd agent on
//! macOS, a systemd user timer on Linux.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, IoContext, Result};
use crate::util::{display_path, home_dir, write_atomic};

const LABEL: &str = "tiger.sync";
const UNIT: &str = "tiger-sync";

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub installed: bool,
    pub path: String,
    pub log: String,
}

/// Where job files go. `TIGER_SCHEDULER_DIR` redirects them and skips
/// loading the job, so tests never touch the real service manager.
fn job_dir() -> Result<(PathBuf, bool)> {
    if let Some(dir) = std::env::var_os("TIGER_SCHEDULER_DIR").filter(|v| !v.is_empty()) {
        return Ok((PathBuf::from(dir), false));
    }
    let home = home_dir().ok_or_else(|| Error::new("could not find the home directory"))?;
    if cfg!(target_os = "macos") {
        Ok((home.join("Library/LaunchAgents"), true))
    } else {
        let config =
            std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        Ok((config.join("systemd/user"), true))
    }
}

fn job_file(dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") { dir.join(format!("{LABEL}.plist")) } else { dir.join(format!("{UNIT}.timer")) }
}

fn log_path(store_root: &Path) -> PathBuf {
    store_root.join("logs").join("schedule.log")
}

pub fn status(store_root: &Path) -> Result<JobStatus> {
    let (dir, _) = job_dir()?;
    let file = job_file(&dir);
    Ok(JobStatus { installed: file.is_file(), path: display_path(&file), log: display_path(&log_path(store_root)) })
}

/// The `tiger` to run: the one on PATH when it is this binary (it survives
/// upgrades), else this binary's own path.
fn program() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| Error::new("could not locate the tiger binary").context(format!("cause: {e}")))?;
    let real = exe.canonicalize().unwrap_or_else(|_| exe.clone());
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("tiger");
            if candidate.canonicalize().is_ok_and(|c| c == real) {
                return Ok(candidate);
            }
        }
    }
    Ok(exe)
}

/// The environment the job needs to find git, the store, and settings.
/// Tokens are never written into job files; release notes in the job use
/// `gh auth token` when `gh` is signed in.
fn passthrough_env() -> Vec<(String, String)> {
    ["PATH", "TIGER_HOME", "XDG_CONFIG_HOME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).map(|v| (k.to_string(), v)))
        .collect()
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn install(every_secs: u64, store_root: &Path) -> Result<JobStatus> {
    let (dir, load) = job_dir()?;
    std::fs::create_dir_all(&dir).at("create", &dir)?;
    let log = log_path(store_root);
    std::fs::create_dir_all(log.parent().expect("log has a parent")).at("create", &log)?;
    let program = program()?;
    let workdir = home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let env = passthrough_env();

    if cfg!(target_os = "macos") {
        let env_xml: String = env.iter().map(|(k, v)| format!("    <key>{}</key><string>{}</string>\n", xml(k), xml(v))).collect();
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n\
             \x20 <key>Label</key><string>{LABEL}</string>\n\
             \x20 <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>sync</string>\n    <string>--everywhere</string>\n    <string>--quiet</string>\n  </array>\n\
             \x20 <key>StartInterval</key><integer>{every_secs}</integer>\n\
             \x20 <key>RunAtLoad</key><false/>\n\
             \x20 <key>ProcessType</key><string>Background</string>\n\
             \x20 <key>LowPriorityIO</key><true/>\n\
             \x20 <key>WorkingDirectory</key><string>{}</string>\n\
             \x20 <key>StandardOutPath</key><string>{}</string>\n\
             \x20 <key>StandardErrorPath</key><string>{}</string>\n\
             \x20 <key>EnvironmentVariables</key>\n  <dict>\n{env_xml}  </dict>\n\
             </dict>\n</plist>\n",
            xml(&program.to_string_lossy()),
            xml(&workdir.to_string_lossy()),
            xml(&log.to_string_lossy()),
            xml(&log.to_string_lossy()),
        );
        let file = job_file(&dir);
        if load {
            unload_launchd(&file);
        }
        write_atomic(&file, plist.as_bytes())?;
        if load {
            let domain = format!("gui/{}", unsafe { libc::getuid() });
            run("launchctl", &["bootstrap", &domain, &file.to_string_lossy()])?;
        }
    } else {
        let env_lines: String = env.iter().map(|(k, v)| format!("Environment=\"{k}={}\"\n", v.replace('"', "\\\""))).collect();
        let service = format!(
            "[Unit]\nDescription=tiger: refresh every project's refs\n\n[Service]\nType=oneshot\n{env_lines}WorkingDirectory={}\nExecStart=\"{}\" sync --everywhere --quiet\nStandardOutput=append:{}\nStandardError=append:{}\n",
            workdir.display(),
            program.display(),
            log.display(),
            log.display()
        );
        let timer = format!(
            "[Unit]\nDescription=Run tiger sync --everywhere every {every_secs}s\n\n[Timer]\nOnBootSec=5min\nOnUnitActiveSec={every_secs}s\n\n[Install]\nWantedBy=timers.target\n"
        );
        write_atomic(&dir.join(format!("{UNIT}.service")), service.as_bytes())?;
        write_atomic(&dir.join(format!("{UNIT}.timer")), timer.as_bytes())?;
        if load {
            run("systemctl", &["--user", "daemon-reload"])?;
            run("systemctl", &["--user", "enable", "--now", &format!("{UNIT}.timer")])?;
            // Restart so a changed interval takes effect now.
            run("systemctl", &["--user", "restart", &format!("{UNIT}.timer")])?;
        }
    }
    status(store_root)
}

pub fn remove(store_root: &Path) -> Result<JobStatus> {
    let (dir, load) = job_dir()?;
    if cfg!(target_os = "macos") {
        let file = job_file(&dir);
        if load {
            unload_launchd(&file);
        }
        let _ = std::fs::remove_file(&file);
    } else {
        if load && dir.join(format!("{UNIT}.timer")).is_file() {
            let _ = run("systemctl", &["--user", "disable", "--now", &format!("{UNIT}.timer")]);
        }
        let _ = std::fs::remove_file(dir.join(format!("{UNIT}.timer")));
        let _ = std::fs::remove_file(dir.join(format!("{UNIT}.service")));
        if load {
            let _ = run("systemctl", &["--user", "daemon-reload"]);
        }
    }
    status(store_root)
}

fn unload_launchd(file: &Path) {
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .stderr(std::process::Stdio::null())
        .status();
    let _ = file;
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new(program).args(args).output().map_err(|e| {
        Error::new(format!("could not run {program}"))
            .context(format!("cause: {e}"))
            .hint("set schedule to off and run `tiger sync --everywhere` from cron instead")
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::new(format!("{program} {} failed", args.join(" "))).context(String::from_utf8_lossy(&output.stderr).trim().to_string()))
}
