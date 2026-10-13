//! Filesystem work: copy-on-write tree clones, atomic swaps, filters, and
//! read-only files.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};

/// How a copy was made: sharing the store's blocks, or a full copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CopyMode {
    Clone,
    Copy,
    /// The copy is the project's own git clone (`history = true`).
    Git,
}

/// `CLONE_NOOWNERCOPY` from <sys/clonefile.h>: don't copy ownership.
#[cfg(target_os = "macos")]
const CLONE_NOOWNERCOPY: u32 = 0x0002;

fn cstr(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

/// Copies the directory `src` to `dst` (which must not exist), sharing blocks
/// when the filesystem can.
pub fn clone_tree(src: &Path, dst: &Path) -> io::Result<CopyMode> {
    #[cfg(target_os = "macos")]
    {
        // clonefile(2) clones a whole hierarchy in one call on APFS.
        let (s, d) = (cstr(src)?, cstr(dst)?);
        let rc = unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), CLONE_NOOWNERCOPY) };
        if rc == 0 {
            return Ok(CopyMode::Clone);
        }
        let err = io::Error::last_os_error();
        let _ = std::fs::remove_dir_all(dst);
        if !matches!(err.raw_os_error(), Some(libc::EXDEV) | Some(libc::ENOTSUP) | Some(libc::EEXIST) | Some(libc::EINVAL)) {
            return Err(err);
        }
    }
    let mut all_cloned = true;
    copy_walk(src, dst, &mut all_cloned)?;
    Ok(if all_cloned { CopyMode::Clone } else { CopyMode::Copy })
}

fn copy_walk(src: &Path, dst: &Path, all_cloned: &mut bool) -> io::Result<()> {
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_walk(&from, &to, all_cloned)?;
        } else if file_type.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else if !clone_file(&from, &to)? {
            *all_cloned = false;
            std::fs::copy(&from, &to)?;
        }
    }
    let mode = std::fs::metadata(src)?.permissions().mode();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode | 0o700))
}

/// Clones one file; `Ok(false)` when the filesystem can't share blocks here.
fn clone_file(from: &Path, to: &Path) -> io::Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let (s, d) = (cstr(from)?, cstr(to)?);
        let rc = unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), CLONE_NOOWNERCOPY) };
        Ok(rc == 0)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let source = std::fs::File::open(from)?;
        let target = std::fs::File::create(to)?;
        let rc = unsafe { libc::ioctl(target.as_raw_fd(), libc::FICLONE, source.as_raw_fd()) };
        if rc == 0 {
            std::fs::set_permissions(to, source.metadata()?.permissions())?;
            return Ok(true);
        }
        drop(target);
        std::fs::remove_file(to)?;
        Ok(false)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (from, to);
        Ok(false)
    }
}

/// Atomically exchanges two existing paths. Returns `false` when the
/// filesystem has no atomic exchange, so the caller can fall back.
fn exchange(a: &Path, b: &Path) -> io::Result<bool> {
    let (ca, cb) = (cstr(a)?, cstr(b)?);
    #[cfg(target_os = "macos")]
    let rc = unsafe { libc::renamex_np(ca.as_ptr(), cb.as_ptr(), libc::RENAME_SWAP) };
    #[cfg(target_os = "linux")]
    let rc = unsafe {
        libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, ca.as_ptr(), libc::AT_FDCWD, cb.as_ptr(), libc::RENAME_EXCHANGE) as libc::c_int
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let rc: libc::c_int = {
        let _ = (ca, cb);
        return Ok(false);
    };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOTSUP) | Some(libc::ENOSYS) => Ok(false),
        _ => Err(err),
    }
}

/// Puts `fresh` at `dest`. When `dest` already exists, the two are swapped
/// in one step and the old content is returned at the path `fresh` had, for
/// the caller to delete.
pub fn replace(fresh: &Path, dest: &Path) -> io::Result<Option<PathBuf>> {
    if std::fs::symlink_metadata(dest).is_err() {
        std::fs::rename(fresh, dest)?;
        return Ok(None);
    }
    if exchange(fresh, dest)? {
        return Ok(Some(fresh.to_path_buf()));
    }
    // No atomic exchange: two renames, with a moment where dest is missing.
    let old = fresh.with_file_name(crate::util::unique_name(".tiger-old"));
    std::fs::rename(dest, &old)?;
    if let Err(err) = std::fs::rename(fresh, dest) {
        let _ = std::fs::rename(&old, dest);
        return Err(err);
    }
    Ok(Some(old))
}

/// Removes a file or directory tree, including read-only files.
pub fn remove(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
        Ok(meta) if meta.is_dir() => {
            make_dirs_writable(path);
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
    }
}

fn make_dirs_writable(dir: &Path) {
    if let Ok(meta) = std::fs::metadata(dir) {
        let mode = meta.permissions().mode();
        if mode & 0o200 == 0 {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode | 0o700));
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                make_dirs_writable(&entry.path());
            }
        }
    }
}

/// Removes write permission from every file under `dir`; directories stay
/// writable so syncs can replace and filter files.
pub fn make_read_only(dir: &Path, skip: Option<&str>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if skip.is_some_and(|s| entry.file_name().as_bytes() == s.as_bytes()) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            make_read_only(&entry.path(), None)?;
        } else if file_type.is_file() {
            let path = entry.path();
            let mode = entry.metadata()?.permissions().mode();
            if mode & 0o222 != 0 {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & !0o222))?;
            }
        }
    }
    Ok(())
}

/// Compiled `include` / `exclude` globs, matched against paths relative to the copy.
pub struct Filters {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

impl Filters {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Filters, globset::Error> {
        let build = |globs: &[String]| -> Result<Option<GlobSet>, globset::Error> {
            if globs.is_empty() {
                return Ok(None);
            }
            let mut builder = GlobSetBuilder::new();
            for glob in globs {
                builder.add(Glob::new(glob)?);
            }
            builder.build().map(Some)
        };
        Ok(Filters { include: build(include)?, exclude: build(exclude)? })
    }

    pub fn is_empty(&self) -> bool {
        self.include.is_none() && self.exclude.is_none()
    }

    fn keeps(&self, rel: &Path) -> bool {
        self.include.as_ref().is_none_or(|set| set.is_match(rel)) && !self.exclude.as_ref().is_some_and(|set| set.is_match(rel))
    }

    /// Deletes every file the filters drop, then every directory left empty.
    pub fn apply(&self, root: &Path) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        self.apply_in(root, Path::new(""))?;
        Ok(())
    }

    /// Returns whether `dir` is empty afterwards.
    fn apply_in(&self, root: &Path, rel_dir: &Path) -> io::Result<bool> {
        let dir = root.join(rel_dir);
        let mut empty = true;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let rel = rel_dir.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                if self.apply_in(root, &rel)? {
                    std::fs::remove_dir(entry.path())?;
                } else {
                    empty = false;
                }
            } else if self.keeps(&rel) {
                empty = false;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path, files: &[&str]) {
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, file).unwrap();
        }
    }

    #[test]
    fn filters_keep_included_and_drop_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        tree(tmp.path(), &["src/a.ts", "src/a.snap", "docs/x.md", "assets/big.png", "README.md"]);
        Filters::new(&["src/**".into(), "docs/**".into()], &["**/*.snap".into()]).unwrap().apply(tmp.path()).unwrap();
        assert!(tmp.path().join("src/a.ts").exists());
        assert!(tmp.path().join("docs/x.md").exists());
        assert!(!tmp.path().join("src/a.snap").exists());
        assert!(!tmp.path().join("assets").exists(), "emptied folders are removed");
        assert!(!tmp.path().join("README.md").exists());
    }

    #[test]
    fn clones_swaps_and_removes_read_only_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, a, b) = (tmp.path().join("src"), tmp.path().join("a"), tmp.path().join("b"));
        tree(&src, &["x/y.txt", "z.txt"]);
        std::os::unix::fs::symlink("z.txt", src.join("link")).unwrap();
        make_read_only(&src, None).unwrap();
        clone_tree(&src, &a).unwrap();
        assert_eq!(std::fs::read_to_string(a.join("x/y.txt")).unwrap(), "x/y.txt");
        assert!(std::fs::write(a.join("z.txt"), "edit").is_err(), "copies are read-only");
        std::fs::create_dir(&b).unwrap();
        std::fs::write(b.join("old.txt"), "old").unwrap();
        let old = replace(&a, &b).unwrap().unwrap();
        assert!(b.join("z.txt").exists());
        assert!(old.join("old.txt").exists());
        remove(&old).unwrap();
        assert!(!old.exists());
    }
}
