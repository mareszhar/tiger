//! `TIGER.md`: orients an agent that lands in the refs folder without context.

use crate::config::{FILE, Project};
use crate::error::Result;

pub const FILE_NAME: &str = "TIGER.md";

pub fn contents(project: &Project) -> String {
    let dir = &project.config.dir;
    let up = "../".repeat(dir.split('/').count());
    format!(
        "# {dir}/\n\
         \n\
         Read-only copies of other repos, managed by [tiger](https://github.com/mareszhar/tiger) and listed in [`{FILE}`]({up}{FILE}), with notes about each as comments above its entry.\n\
         \n\
         - **Read and search freely.** Each folder is one ref: `rg \"pattern\" {dir}/<id>`.\n\
         - **Don't edit files in ref folders.** They are read-only, and every sync replaces them.\n\
         - **Refresh with `tiger sync`;** `tiger status` shows which refs are behind upstream, and `tiger show <id>` which commit a copy holds.\n\
         - **Add or remove refs with `tiger add <url>` and `tiger rm <id>`,** or by editing `{FILE}`.\n\
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
