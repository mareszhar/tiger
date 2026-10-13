//! Global settings: `$XDG_CONFIG_HOME/tiger/config.toml`, default
//! `~/.config/tiger/config.toml`. They live apart from the store because
//! settings are kept and the store is disposable.

use std::path::PathBuf;

use toml_edit::{DocumentMut, value};

use crate::config::validate_dir;
use crate::error::{Error, IoContext, Result};
use crate::util::{closest, parse_interval};

pub const DEFAULT_CONCURRENCY: usize = 32;
pub const MIN_SCHEDULE_SECS: u64 = 15 * 60;

/// Every setting: key, default, meaning.
pub const KEYS: &[(&str, &str, &str)] = &[
    ("dir", "tiger", "refs folder that `tiger init` writes into new configs"),
    ("store", "~/.tiger", "where the shared store lives ($TIGER_HOME overrides it)"),
    ("concurrency", "32", "network operations run at once"),
    ("schedule", "off", "run `tiger sync --everywhere` in the background: off, or an interval such as 1h, 6h, 1d (minimum 15m)"),
];

#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub dir: Option<String>,
    pub store: Option<String>,
    pub concurrency: Option<usize>,
    pub schedule: Option<String>,
}

impl Settings {
    pub fn concurrency(&self) -> usize {
        self.concurrency.unwrap_or(DEFAULT_CONCURRENCY)
    }

    pub fn dir(&self) -> &str {
        self.dir.as_deref().unwrap_or(crate::config::DEFAULT_DIR)
    }

    /// The schedule interval in seconds, or `None` when off.
    pub fn schedule_secs(&self) -> Option<u64> {
        self.schedule.as_deref().filter(|s| *s != "off").and_then(parse_interval)
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "dir" => self.dir.clone(),
            "store" => self.store.clone(),
            "concurrency" => self.concurrency.map(|c| c.to_string()),
            "schedule" => self.schedule.clone(),
            _ => None,
        }
    }
}

pub fn path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir).join("tiger").join("config.toml"));
    }
    crate::util::home_dir()
        .map(|h| h.join(".config").join("tiger").join("config.toml"))
        .ok_or_else(|| Error::new("could not find the home directory").hint("set XDG_CONFIG_HOME"))
}

fn read_doc() -> Result<(PathBuf, DocumentMut)> {
    let path = path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(Error::io("read", &path, err)),
    };
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|e| Error::new(format!("{} is not valid TOML", path.display())).context(e.to_string().trim().replace('\n', "\n  ")))?;
    Ok((path, doc))
}

pub fn load() -> Result<Settings> {
    let (path, doc) = read_doc()?;
    let mut settings = Settings::default();
    for (key, item) in doc.iter() {
        let text = item.as_str().map(str::to_string).or_else(|| item.as_integer().map(|i| i.to_string()));
        let Some(text) = text else {
            return Err(Error::new(format!("invalid setting '{key}' in {}", path.display()))
                .hint(format!("set it with `tiger config set {key} <value>`")));
        };
        apply(&mut settings, key, &text).map_err(|e| e.context(format!("in {}", path.display())))?;
    }
    Ok(settings)
}

fn apply(settings: &mut Settings, key: &str, text: &str) -> Result<()> {
    match key {
        "dir" => {
            validate_dir(text).map_err(|reason| Error::new(format!("invalid dir: {reason}")))?;
            settings.dir = Some(text.to_string());
        }
        "store" => settings.store = Some(text.to_string()),
        "concurrency" => match text.parse::<usize>() {
            Ok(n) if (1..=64).contains(&n) => settings.concurrency = Some(n),
            _ => return Err(Error::new(format!("invalid concurrency {text:?}")).hint("use a number from 1 to 64")),
        },
        "schedule" => {
            if text != "off" {
                match parse_interval(text) {
                    Some(secs) if secs >= MIN_SCHEDULE_SECS => {}
                    Some(_) => return Err(Error::new(format!("schedule {text:?} is too frequent")).hint("use 15m or longer")),
                    None => {
                        return Err(Error::new(format!("invalid schedule {text:?}")).hint("use off, or an interval such as 30m, 6h, or 1d"));
                    }
                }
            }
            settings.schedule = Some(text.to_string());
        }
        other => return Err(unknown(other)),
    }
    Ok(())
}

pub fn unknown(key: &str) -> Error {
    let keys: Vec<&str> = KEYS.iter().map(|(k, _, _)| *k).collect();
    let mut err = Error::new(format!("unknown setting '{key}'")).context(format!("settings: {}", keys.join(", ")));
    if let Some(close) = closest(key, &keys) {
        err = err.hint(format!("did you mean '{close}'?"));
    }
    err
}

/// Validates and writes one setting, keeping the file's comments.
pub fn set(key: &str, text: &str) -> Result<Settings> {
    let mut settings = load()?;
    apply(&mut settings, key, text)?;
    let (path, mut doc) = read_doc()?;
    if key == "concurrency" {
        doc[key] = value(text.parse::<i64>().expect("validated"));
    } else {
        doc[key] = value(text);
    }
    write(&path, &doc)?;
    Ok(settings)
}

pub fn unset(key: &str) -> Result<Settings> {
    if !KEYS.iter().any(|(k, _, _)| *k == key) {
        return Err(unknown(key));
    }
    let (path, mut doc) = read_doc()?;
    if doc.remove(key).is_some() {
        write(&path, &doc)?;
    }
    load()
}

fn write(path: &std::path::Path, doc: &DocumentMut) -> Result<()> {
    let dir = path.parent().expect("settings path has a parent");
    std::fs::create_dir_all(dir).at("create", dir)?;
    crate::util::write_atomic(path, doc.to_string().as_bytes())
}
