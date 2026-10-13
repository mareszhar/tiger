//! Edits to `tiger.toml` that keep every comment and all formatting.
//! Notes about a ref are the comments above its entry, so they travel with it.

use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value, value};

use crate::config;
use crate::error::{Error, IoContext, Result};

pub fn load(path: &Path) -> Result<DocumentMut> {
    let text = std::fs::read_to_string(path).at("read", path)?;
    text.parse()
        .map_err(|e: toml_edit::TomlError| Error::new("tiger.toml is not valid TOML").context(e.to_string().trim().replace('\n', "\n  ")))
}

/// Writes the document after checking it is still a valid config.
pub fn save(path: &Path, doc: &DocumentMut) -> Result<()> {
    let text = doc.to_string();
    config::parse(&text).map_err(|e| e.context("the edit was not saved"))?;
    crate::util::write_atomic(path, text.as_bytes())
}

#[derive(Debug, Clone, Default)]
pub struct NewRef {
    pub id: String,
    pub url: String,
    pub git_ref: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub history: bool,
    pub releases: bool,
    /// `Some(false)` writes `propagate = false`.
    pub propagate: Option<bool>,
    pub note: Option<String>,
}

impl NewRef {
    fn is_url_only(&self) -> bool {
        self.git_ref.is_none()
            && self.include.is_empty()
            && self.exclude.is_empty()
            && !self.history
            && !self.releases
            && self.propagate.is_none()
    }
}

fn refs_table(doc: &mut DocumentMut) -> Result<&mut dyn toml_edit::TableLike> {
    if doc.get("refs").is_none() {
        doc.insert("refs", Item::Table(Table::new()));
    }
    doc.get_mut("refs")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| Error::new("refs in tiger.toml is not a table").hint("make it a [refs] table"))
}

/// `# note` lines wrapped at 78 columns, with a blank line before the entry.
fn comment_prefix(note: Option<&str>) -> String {
    let mut prefix = String::from("\n");
    if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
        for paragraph in note.lines() {
            let mut line = String::from("#");
            for word in paragraph.split_whitespace() {
                if line.len() + 1 + word.len() > 78 && line.len() > 1 {
                    prefix.push_str(&line);
                    prefix.push('\n');
                    line = String::from("#");
                }
                line.push(' ');
                line.push_str(word);
            }
            prefix.push_str(&line);
            prefix.push('\n');
        }
    }
    prefix
}

fn string_array(items: &[String]) -> Value {
    let mut array = Array::new();
    for item in items {
        array.push(item.as_str());
    }
    Value::Array(array)
}

pub fn add(doc: &mut DocumentMut, new: &NewRef) -> Result<()> {
    let prefix = comment_prefix(new.note.as_deref());
    if new.is_url_only() {
        let refs = refs_table(doc)?;
        refs.insert(&new.id, value(new.url.as_str()));
        if let Some(mut key) = refs.key_mut(&new.id) {
            key.leaf_decor_mut().set_prefix(prefix);
        }
        if let Some(table) = doc.get_mut("refs").and_then(Item::as_table_mut) {
            table.set_implicit(false);
        }
        return Ok(());
    }
    let mut table = Table::new();
    table.insert("url", value(new.url.as_str()));
    if let Some(r) = &new.git_ref {
        table.insert("ref", value(r.as_str()));
    }
    if !new.include.is_empty() {
        table.insert("include", Item::Value(string_array(&new.include)));
    }
    if !new.exclude.is_empty() {
        table.insert("exclude", Item::Value(string_array(&new.exclude)));
    }
    if new.history {
        table.insert("history", value(true));
    }
    if new.releases {
        table.insert("releases", value(true));
    }
    if let Some(propagate) = new.propagate {
        table.insert("propagate", value(propagate));
    }
    table.decor_mut().set_prefix(prefix);
    match doc.get_mut("refs") {
        Some(Item::Table(refs)) => {
            refs.insert(&new.id, Item::Table(table));
        }
        _ => {
            let refs = refs_table(doc)?;
            let mut inline = table.into_inline_table();
            inline.fmt();
            refs.insert(&new.id, Item::Value(Value::InlineTable(inline)));
        }
    }
    Ok(())
}

pub fn remove(doc: &mut DocumentMut, id: &str) -> Result<bool> {
    Ok(refs_table(doc)?.remove(id).is_some())
}

/// Sets (`Some`) or removes (`None`) a ref's `ref` field.
pub fn set_ref(doc: &mut DocumentMut, id: &str, git_ref: Option<&str>) -> Result<()> {
    let refs = refs_table(doc)?;
    let item = refs.get_mut(id).ok_or_else(|| Error::new(format!("unknown ref '{id}'")))?;
    match item {
        Item::Value(Value::String(url)) => {
            if let Some(r) = git_ref {
                let mut inline = InlineTable::new();
                inline.insert("url", Value::from(url.value().as_str()));
                inline.insert("ref", Value::from(r));
                inline.fmt();
                let decor = url.decor().clone();
                let mut new_value = Value::InlineTable(inline);
                *new_value.decor_mut() = decor;
                *item = Item::Value(new_value);
            }
        }
        Item::Value(Value::InlineTable(inline)) => {
            match git_ref {
                Some(r) => {
                    inline.insert("ref", Value::from(r));
                }
                None => {
                    inline.remove("ref");
                }
            }
            inline.fmt();
            // `{ url = "…" }` alone reads better as the plain string.
            if inline.len() == 1
                && let Some(url) = inline.get("url").and_then(Value::as_str).map(str::to_string)
            {
                let decor = inline.decor().clone();
                let mut new_value = Value::from(url);
                *new_value.decor_mut() = decor;
                *item = Item::Value(new_value);
            }
        }
        Item::Table(table) => match git_ref {
            Some(r) => {
                table.insert("ref", value(r));
            }
            None => {
                table.remove("ref");
            }
        },
        _ => return Err(Error::new(format!("refs.{id} is not a URL or a table"))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# header\n\ndir = \"tiger\"\n\n[refs]\n# Editor framework.\ntiptap = \"https://github.com/ueberdosis/tiptap\"\n\n# Pinned for now.\nzod = { url = \"https://github.com/colinhacks/zod\", ref = \"v3.4.2\" }\n";

    #[test]
    fn adds_entries_with_notes_and_keeps_comments() {
        let mut doc: DocumentMut = DOC.parse().unwrap();
        add(
            &mut doc,
            &NewRef { id: "pm".into(), url: "https://x/pm".into(), note: Some("ProseMirror model.".into()), ..NewRef::default() },
        )
        .unwrap();
        add(
            &mut doc,
            &NewRef {
                id: "big".into(),
                url: "https://x/big".into(),
                include: vec!["src/**".into()],
                note: Some("Only the source.".into()),
                ..NewRef::default()
            },
        )
        .unwrap();
        let text = doc.to_string();
        assert!(text.starts_with(DOC.trim_end()), "{text}");
        assert!(text.contains("\n# ProseMirror model.\npm = \"https://x/pm\"\n"), "{text}");
        assert!(text.contains("\n# Only the source.\n[refs.big]\nurl = \"https://x/big\"\ninclude = [\"src/**\"]\n"), "{text}");
        let config = config::parse(&text).unwrap();
        assert_eq!(config.refs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["tiptap", "zod", "pm", "big"]);
    }

    #[test]
    fn removes_an_entry_with_its_note() {
        let mut doc: DocumentMut = DOC.parse().unwrap();
        assert!(remove(&mut doc, "tiptap").unwrap());
        let text = doc.to_string();
        assert!(!text.contains("Editor framework"), "{text}");
        assert!(text.contains("# Pinned for now.\nzod"), "{text}");
    }

    #[test]
    fn pins_and_unpins_in_place() {
        let mut doc: DocumentMut = DOC.parse().unwrap();
        set_ref(&mut doc, "tiptap", Some("v2.0.0")).unwrap();
        set_ref(&mut doc, "zod", None).unwrap();
        let text = doc.to_string();
        assert!(
            text.contains("# Editor framework.\ntiptap = { url = \"https://github.com/ueberdosis/tiptap\", ref = \"v2.0.0\" }\n"),
            "{text}"
        );
        assert!(text.contains("# Pinned for now.\nzod = \"https://github.com/colinhacks/zod\"\n"), "{text}");
    }

    #[test]
    fn wraps_long_notes() {
        let prefix = comment_prefix(Some(&"word ".repeat(30)));
        assert!(prefix.lines().all(|l| l.len() <= 78));
        assert!(prefix.lines().skip(1).all(|l| l.starts_with("# ")));
    }
}
