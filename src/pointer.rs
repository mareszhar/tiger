//! `TIGER.md`: orients an agent that lands in the refs folder without context.

use crate::config::{FILE, Project};
use crate::error::Result;

pub const FILE_NAME: &str = "TIGER.md";

pub fn contents(project: &Project) -> String {
    let dir = &project.config.dir;
    let title = if dir == "." { "Refs".to_string() } else { format!("{dir}/") };
    let base = project.dir_path();
    let config = crate::util::relative_path(&base, &project.config_path())
        .display()
        .to_string()
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29");
    let search = if dir == "." { "<id>".to_string() } else { format!("{}/<id>", crate::util::shell_quote(dir)) };
    let sync = project.command_from(&base, "sync");
    let status = project.command_from(&base, "status");
    let show = project.command_from(&base, "show <id>");
    let add = project.command_from(&base, "add <url>");
    let rm = project.command_from(&base, "rm <id>");
    format!(
        "# {title}\n\
         \n\
         Read-only copies of other repos, managed by [tiger](https://github.com/mareszhar/tiger) and listed in [`{FILE}`]({config}), with notes about each as comments above its entry.\n\
         \n\
         - **Read and search freely.** Each folder is one ref; search one by passing its path, like `rg \"pattern\" {search}` from the folder with `{FILE}`.\n\
         - **Don't edit files in ref folders.** They are read-only, and every sync replaces them.\n\
         - **Refresh with `{sync}`;** `{status}` shows which refs are behind upstream, and `{show}` which commit a copy holds.\n\
         - **Add or remove refs with `{add}` and `{rm}`,** or by editing the linked config.\n\
         - Files here that aren't ref folders are left alone by tiger.\n"
    )
}

/// Writes `TIGER.md` when it is missing or out of date.
pub fn write(project: &Project) -> Result<()> {
    let path = project.dir_path().join(FILE_NAME);
    let wanted = contents(project);
    if std::fs::read_to_string(&path).is_ok_and(|current| current == wanted) {
        return Ok(());
    }
    crate::util::write_atomic(&path, wanted.as_bytes())
}
