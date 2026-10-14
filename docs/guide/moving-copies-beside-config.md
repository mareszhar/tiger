# Moving copies beside their config

An advanced recipe for one layout: `__references__/tiger.toml` with `dir = "tiger"`, whose copies you want beside the config with `dir = "."`. It moves the existing copies and tiger's records about them instead of rebuilding them, so it keeps what a rebuild can't: refs pinned to a tag whose repo is no longer reachable, `history = true` clones without cloning again, and each copy's copied time.

For most moves, [rebuilding the copies](./getting-started.md#11-moving-copies-to-another-folder) is simpler. Neither is a general way to change `dir`.

Before you start:

- **Stop other syncs.** Note `tiger config get schedule`; if a schedule is on, run `tiger config set schedule off`, and turn it back on only after checking the result. Nothing may sync during the move.
- **Check your ignore rules.** A `/tiger/` rule won't cover copies moved beside the config.

Run the script below from `__references__/` (it needs Python 3). It checks every destination before moving anything, moves tiger's records with the copies, keeps your own files in `tiger/`, and changes only the exact `dir = "tiger"` line `tiger init` wrote. If you've reformatted that line, write it that way first.

<!-- relocation-script -->

```sh
python3 - <<'PY'
import json
import os
from pathlib import Path
import tempfile

root = Path.cwd()
config = root / "tiger.toml"
source = root / "tiger"
text = config.read_text()
if text.splitlines().count('dir = "tiger"') != 1 or source.is_symlink():
    raise SystemExit('expected dir = "tiger" and an ordinary tiger/ folder')
state = json.loads((source / ".tiger-state.json").read_text())
ids = list(state["refs"])
if any(Path(name).name != name or name in ("", ".", "..") for name in ids):
    raise SystemExit("invalid ownership state; nothing moved")
names = set(ids) | {name + ".releases.md" for name in ids}
names |= {".tiger-state.json", ".tiger-lock", ".tiger-store", "TIGER.md"}
paths = [source / name for name in sorted(names)
         if (source / name).exists() or (source / name).is_symlink()]
if any(p.name.startswith(".tiger-tmp-") for p in source.iterdir()):
    raise SystemExit("inspect unfinished sync work before moving")
if any(p.is_symlink() or (root / p.name).exists()
       or (root / p.name).is_symlink() for p in paths):
    raise SystemExit("a source is a symlink or a destination exists; nothing moved")
updated = "".join(line.replace('dir = "tiger"', 'dir = "."', 1)
                  if line.rstrip("\r\n") == 'dir = "tiger"' else line
                  for line in text.splitlines(keepends=True))
with tempfile.NamedTemporaryFile(mode="w", dir=root,
                                 prefix=".tiger-tmp-relocate-", delete=False) as file:
    file.write(updated)
    temporary = Path(file.name)
os.chmod(temporary, config.stat().st_mode & 0o777)
moved = []
try:
    for path in paths:
        target = root / path.name
        path.rename(target)
        moved.append((path, target))
    temporary.replace(config)
except BaseException:
    for original, target in reversed(moved):
        target.rename(original)
    raise
finally:
    temporary.unlink(missing_ok=True)
PY
```

Then run `tiger sync` and `tiger status` from the same folder. Sync rewrites `TIGER.md` and, when the shared store is writable, promotes any moved fallback data. Keep the old `tiger/` folder if it still holds files of yours.

If the script is killed partway (not just failing, which undoes itself) and `tiger.toml` still says `dir = "tiger"`, move whatever it moved back into `tiger/` before trying again, and don't sync in between.
