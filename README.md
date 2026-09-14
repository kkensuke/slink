# slink

A managed symbolic-link CLI for macOS. Create links and record their intended
targets in a hand-editable TOML file, then inspect, restore, or remove them.

## Install

With a current stable Rust toolchain:

While the initial PR is open, install its branch:

```sh
cargo install --git https://github.com/kkensuke/slink.git --branch feat/slink-cli --locked
```

After it is merged, install from the repository's default branch:

```sh
git clone https://github.com/kkensuke/slink.git
cd slink
cargo install --path . --locked
```

Successful macOS CI jobs also provide a release executable as an Actions artifact.
The supported user platform is macOS; Linux CI exercises the portable logic.

## Commands

```sh
slink ~/dotfiles/zshrc ~/.zshrc
slink --relative --parents ~/dotfiles/nvim ~/.config/nvim
slink list
slink check
slink fix --dry-run
slink fix --parents
slink fix --replace ~/.zshrc
slink remove ~/.zshrc
slink remove --keep-link ~/.config/nvim
slink adopt ~/.gitconfig ~/.tmux.conf
slink scan ~/.config
```

| Command | Responsibility |
| --- | --- |
| `slink <target> <link>` | Create and register a new symlink |
| `list` | Display registrations, without inspecting their destinations |
| `check [link ...]` | Inspect registered links and target availability |
| `fix [link ...]` | Restore missing links; replace mismatches only with `--replace` |
| `remove <link ...>` | Delete matching symlinks and unregister them |
| `adopt <link ...>` | Register existing symlinks without changing them |
| `scan <directory ...>` | Discover symlinks without registering them |

`check` and `fix` default to all entries in the selected registry. `remove` and
`adopt` require explicit paths. `scan` recurses into ordinary directories and
never follows symlink directories; its roots must also be ordinary directories.
Use `--` for operands that begin with a dash or match a command name:

```sh
slink -- list ./list-link
slink check -- ./list-link
```

## Registry

The default is `$XDG_CONFIG_HOME/slink/links.toml`, falling back to
`~/.config/slink/links.toml` when the variable is unset, empty, or relative.
`--file` selects exactly one other file. Files are never merged or auto-discovered.

```sh
slink --file ./project-links.toml check
```

```toml
version = 1

# Shell configuration
[[links]]
link = "~/.zshrc"
target = "dotfiles/zshrc"

# Editor configuration
[[links]]
link = "~/.config/nvim"
target = "../dotfiles/nvim"
```

`link` is the link's location; `target` is the exact string stored inside it.
Relative registry `link` paths are based on the selected registry's directory.
Only `link` supports a leading `~/` expansion. Registry `target` strings do not
expand `~`, variables, or shell expressions. Relative targets are resolved from
the link's parent, never the registry directory.

CLI registration writes an absolute `link` path. Handwritten path spelling,
comments, ordering, and quote styles are preserved when other entries change.
A registry that is itself a symlink is updated through its referent; the registry
symlink is retained. The selected registry location still determines relative
`link` paths in that file.

- Adding an entry lets `fix` create its missing link.
- Editing a target requires `fix --replace` to change an existing mismatched link.
- Removing an entry only unregisters it; its symlink remains.
- Editing a link location unregisters the old location, whose symlink remains.

## Explicit options and defaults

`--relative` and `--parents` are opt-in. With no `--relative`, target operands
are stored literally, as with `ln -s`. With `--relative`, the target operand is
interpreted from the invocation directory and converted into a link-relative path.
This conversion preserves target-side symlinks and meaningful `..` components.

`--parents` creates missing **link parent directories**, never target directories.
Removing links does not remove their parents. No `relative` or `parents` settings
are duplicated in the registry; the stored target already captures the result.

`--dry-run` works for create, fix, remove, and adopt, and makes no writes, including
lock files, recovery records, or parent directories. `--keep-link` only works with
remove; `--replace` only works with fix. Invalid option combinations are errors.
Batch previews account for earlier planned registrations/removals. Recovery
previews check registry edits and destination conflicts before showing a plan.

See [the default-behavior decisions](docs/defaults.md) for the tradeoffs.

## Safety and recovery

Missing targets are allowed and reported. Ordinary files/directories are never
overwritten by `fix --replace` or deleted by `remove`. An unregistered existing
symlink must be adopted first. A changed managed link must be explicitly replaced
or unregistered with `--keep-link`.
The selected registry, its control files, and their parent paths cannot themselves
be managed destinations. Put the registry elsewhere with `--file` if you need to
manage a directory that would contain it.

Mutation commands use a registry lock and compare registry contents again before
saving. They retain a small operation record if a create/register, remove, or
replacement operation is interrupted. Repeat the operation for the failed link
with the same target and options to resume. `check` identifies the pending link;
for a partly completed removal batch, omit paths already unregistered by earlier
items. Unrelated
mutations stop until recovery is resolved. Recovery rechecks the actual files; it
does not overwrite a conflicting manual edit.

Replacement/removal stages the old link in a private directory beside its original
location, verifies its identity, and deletes only that verified symlink. Unexpected
objects are preserved. Replacement may briefly leave the link name absent.

The registry may have adjacent `.slink-lock` and, while an operation is incomplete,
`.slink-pending` files. Do not delete pending files or `.slink-*` recovery directories
before resolving an interrupted operation. Locks coordinate slink processes, not
arbitrary editors. There is no whole-batch atomicity or unconditional power-loss
guarantee; completed items are retained when a later item fails.

Initial boundaries: UTF-8 paths and target text; no nested managed destinations;
ambiguous destination spellings are rejected conservatively. A missing parent
containing `..` must be resolved explicitly rather than guessed. Filesystem
operations that cannot provide the required exclusive behavior fail rather than
falling back to overwriting existing paths.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Requested operation completed; for check, all selected entries are healthy |
| 1 | Check found a problem, an item failed/conflicted, or traversal was incomplete |
| 2 | Invalid arguments/registry, unavailable registry, or a blocking recovery error |

Creating or fixing a link successfully returns 0 even if its target is missing.
`check` returns 1 for that target. `scan` does not fail merely for finding a broken
link. A mismatched link left unrepaired by normal `fix` returns 1.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

Integration tests run the real executable with isolated registries and include
abrupt interruption/recovery at each mutation stage. Failure injection is only
compiled into debug builds; release executables ignore the test crash variable.
