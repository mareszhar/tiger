//! Sync activity on stderr: a line when work starts, a line when a step
//! with real work begins, then an update only when it says something new:
//! at most every 5 seconds when the step advanced, and every 15 seconds when
//! nothing did, so a long fetch or a lock wait never looks like a hang.
//! Bookkeeping steps (locks, state) only show up through those updates, when
//! they are slow enough to matter.

use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// An update when the step advanced and this long passed since the last line.
const PROGRESS_EVERY: Duration = Duration::from_secs(5);
/// An update even when nothing advanced, so the process never looks hung.
const ALIVE_EVERY: Duration = Duration::from_secs(15);

/// Whether a heartbeat should print: something new to say, or a long silence.
fn update_due(since_last_line: Duration, advanced: bool) -> bool {
    since_last_line >= ALIVE_EVERY || (advanced && since_last_line >= PROGRESS_EVERY)
}

#[derive(Clone)]
pub struct Activity {
    state: Arc<Mutex<ActivityState>>,
    enabled: bool,
}

struct ActivityState {
    started: Instant,
    /// Nothing prints until `begin`, so config errors stay the only output.
    begun: bool,
    /// What the heartbeat says is happening, e.g. "fetching commits".
    label: String,
    done: usize,
    total: usize,
    last_print: Instant,
    /// `done` as of the last printed line, to tell whether the step advanced.
    printed_done: usize,
}

impl Activity {
    /// The first line: how much this sync covers. Called before any lock.
    pub fn begin(&self, refs: usize, projects: usize) {
        let mut state = self.state.lock().unwrap();
        state.begun = true;
        state.label = "starting".into();
        if self.enabled {
            let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
            let scope = if projects > 1 { format!(" across {}", plural(projects, "project")) } else { String::new() };
            state.print(&format!("syncing {}{scope}", plural(refs, "ref")));
        }
    }

    /// A phase with visible work: announced now, e.g. "fetching 3 commits".
    pub fn step(&self, label: &str, total: usize, announcement: &str) {
        let mut state = self.state.lock().unwrap();
        state.set(label, total);
        if self.enabled && total > 0 {
            state.print(announcement);
        }
    }

    /// A bookkeeping phase: silent unless it outlasts the heartbeat.
    pub fn quietly(&self, label: &str, total: usize) {
        self.state.lock().unwrap().set(label, total);
    }

    pub fn done(&self) {
        self.state.lock().unwrap().done += 1;
    }

    /// Something worth saying right away, such as a retry.
    pub fn message(&self, message: &str) {
        if self.enabled {
            self.state.lock().unwrap().print(message);
        }
    }
}

impl ActivityState {
    fn set(&mut self, label: &str, total: usize) {
        self.label = label.to_string();
        self.done = 0;
        self.total = total;
        self.printed_done = 0;
    }

    fn print(&mut self, text: &str) {
        eprintln!("tiger: {text}");
        self.last_print = Instant::now();
    }

    fn heartbeat(&mut self) {
        if !self.begun || !update_due(self.last_print.elapsed(), self.done != self.printed_done) {
            return;
        }
        self.printed_done = self.done;
        let counts = if self.total > 1 { format!(" · {}/{} done", self.done, self.total) } else { String::new() };
        let elapsed = crate::util::format_duration_ms(self.started.elapsed().as_millis());
        let text = format!("{}{counts} · {elapsed}", self.label);
        self.print(&text);
    }
}

pub struct Progress {
    pub activity: Activity,
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Progress {
    pub fn start(quiet: bool) -> Self {
        let activity = Activity {
            enabled: !quiet,
            state: Arc::new(Mutex::new(ActivityState {
                started: Instant::now(),
                begun: false,
                label: String::new(),
                done: 0,
                total: 0,
                last_print: Instant::now(),
                printed_done: 0,
            })),
        };
        let (stop, receiver) = mpsc::channel();
        let worker = (!quiet).then(|| {
            let state = Arc::clone(&activity.state);
            std::thread::spawn(move || {
                while receiver.recv_timeout(Duration::from_millis(250)) == Err(mpsc::RecvTimeoutError::Timeout) {
                    state.lock().unwrap().heartbeat();
                }
            })
        });
        Self { activity, stop, worker }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_only_when_something_advanced_or_after_a_long_silence() {
        let s = Duration::from_secs;
        assert!(!update_due(s(2), true), "too soon, even with progress");
        assert!(update_due(s(5), true), "progress, 5s after the last line");
        assert!(!update_due(s(10), false), "nothing new to say yet");
        assert!(update_due(s(15), false), "a sign of life after 15s");
    }
}
