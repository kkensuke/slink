# slink

[English](README.md) | [日本語](README.ja.md)

`slink` is a macOS CLI for safely creating and managing symbolic links (symlinks) using a TOML file.

## Install

```sh
brew install kkensuke/tap/slink
```

Or build from source with a current stable Rust toolchain:

```sh
git clone https://github.com/kkensuke/slink.git
cd slink
cargo install --path . --locked
```

## Quick start `slink <target> <link>`

`<target>` is the path to point to, and `<link>` is where to create the symlink. Relative paths are interpreted from the directory in which you run the command (the working directory).

```sh
cd ~
slink -p A/target B/link
```

If your home directory is `/Users/you`, this command works as follows:

- It creates `/Users/you/B/link`, pointing to `/Users/you/A/target`, and records it in the registry file.
- `-p` creates the parent directory `B` if needed. It does not create the target `A/target`.
- A regular file or directory already at `B/link` is left untouched and reported as a conflict. The command does not create a symlink inside an existing directory.
- The target can itself be a symlink. In that case, slink still registers the path you specified as the target.

The registry file contains:

```toml
[[link]]
link   = "/Users/you/B/link"
target = "/Users/you/A/target"
```

To view the registrations:

```sh
slink list
```

With the single registration above, the output is:

```text
1 link

"/Users/you/B/link"
  → "/Users/you/A/target"
```

To check that the symlinks match their registrations and their targets are reachable:

```sh
slink check
```

You can create a symlink to a target that does not exist, but `check` reports a problem until the target becomes reachable.

## Commands

```text
slink [options] <target> <link>
slink --config
slink list
slink check [link ...]
slink fix [options] [link ...]
slink unregister [options] <link ...>
slink remove [options] <link ...>
slink adopt [options] <link ...>
slink scan [options] [directory ...]
```

| Command | Purpose |
| --- | --- |
| `slink <target> <link>` | Create a symlink to the specified target and add or update its registration |
| `slink --config` | Print the registry file's location |
| `slink list` | Display registrations without inspecting symlinks |
| `slink check [link ...]` | Compare symlinks with their registrations and check whether their targets are reachable |
| `slink fix [link ...]` | Restore symlinks from the registry; replacing an existing symlink with a different target requires `-f` |
| `slink unregister <link ...>` | Unregister symlinks without changing anything at their locations |
| `slink remove <link ...>` | Delete symlinks that match their registrations and unregister them; leave target files and directories untouched |
| `slink adopt <link ...>` | Add or update registrations from existing symlinks without changing the symlinks |
| `slink scan [directory ...]` | Find symlinks directly inside the specified directories; defaults to the working directory |

`check` and `fix` process all registrations if no symlink is specified. `unregister`, `remove`, and `adopt` require explicit symlink paths.

### If the location already exists `slink -f <target> <link>`

| What exists at the location | Normal creation | Creation with `-f` |
| --- | --- | --- |
| Nothing | Create the symlink; add or update its registration | Same |
| A symlink with a matching target path | Keep the symlink; add or update its registration | Same |
| A symlink with a different target path | Report a conflict | Replace the symlink; add or update its registration |
| A regular file, directory, or other object | Report a conflict and leave it untouched | Same |

## Paths

| Input or stored value | Rule |
| --- | --- |
| Paths passed to commands | Absolute paths, paths relative to the working directory, and leading `~/` are accepted and converted to absolute paths |
| Registry `link` / `target` | The symlink's location / target path. Both must be absolute paths, including when edited by hand |
| Targets of newly created or restored symlinks | Set using absolute paths |

You can also pass quoted `"~/…"` paths to commands. The registry does not expand `~`, variables, or shell expressions.

### Register an existing symlink that uses a relative path with `adopt`

`adopt` converts a relative path read from an existing symlink to an absolute path, using the directory containing the symlink as its base, and registers it.

For example, if `/Users/you/bin/python3` points to `python`, the registry stores `/Users/you/bin/python`.

```sh
slink adopt ~/bin/python3
slink check ~/bin/python3
slink fix ~/bin/python3
```

`check` and `fix` also convert the target path to an absolute path before comparing it with the registration. A matching symlink is left untouched. If the symlink has been deleted, `fix` recreates it using an absolute path for its target.

## Registry file

The registry is normally `~/.config/slink/links.toml`. If `XDG_CONFIG_HOME` is set to an absolute path, slink uses `$XDG_CONFIG_HOME/slink/links.toml` instead. An unset, empty, or relative value uses the default location.

Creating a symlink or running `adopt` creates the registry file if needed. `scan` can run without a registry file.

To open the registry for viewing or editing:

```sh
open "$(slink --config)"
```

Write one `[[link]]` block per registration, containing only the string fields `link` and `target`. Both must be absolute paths. An empty file represents no registrations.

`list` displays registrations in file order, including entries with invalid paths or duplicate registrations. Use `check` to validate them. `check`, `scan`, and commands that make changes validate paths and check for duplicates across the entire registry. Invalid TOML syntax or entry structure prevents even `list` from reading the file.

| Hand edit | Effect |
| --- | --- |
| Change `link` | The old symlink remains unregistered; `fix` can create the new symlink |
| Change `target` | `fix -f` can update an existing symlink's target |
| Add an entry | `fix` can create the missing symlink |
| Delete the whole entry | The symlink remains and is no longer included in `list`, `check`, or `fix` |

Use `unregister` to stop managing a symlink while keeping it in place. To delete both the symlink and its registration, use `remove` before deleting the entry. To update a registration to match a manually changed symlink, use `adopt`.

If the registry file is itself a symlink, slink updates the file it points to.

## Options

| Short | Long | Purpose |
| --- | --- | --- |
| `-c` | `--config` | Print the registry file's location |
| `-f` | `--force` | Create/`fix`: replace existing symlinks with different targets |
| `-p` | `--parents` | Create/`fix`: create missing parent directories for symlinks |
| `-n` | `--dry-run` | Create/`fix`/`unregister`/`remove`/`adopt`: preview changes without writing |
| `-R` | `--recursive` | `scan`: include subdirectories |
| `-o` | `--format <human\|tsv>` | `list`/`check`/`scan`: choose the output format |
| `-h` | `--help` | Show help |
| `-V` | `--version` | Show the version |

Short flags can be combined, such as `-np`. Use `--config`, `--help`, and `--version` on their own.

To pass a path with the same name as a command or option, put `--` before it:

```sh
slink -- list ./list-link
slink check -- -link
```

The first command uses the file `list` in the working directory as its target. The second checks the registered symlink named `-link`.

## Find symlinks with `scan`

```sh
slink scan
slink scan -R ~/projects
slink scan ~/links
```

- `scan` searches directly inside the specified directories. It defaults to the working directory when none is specified. Add `-R` to include subdirectories. Symlinks to directories are displayed, but their contents are not scanned. You also cannot use a symlink as a starting directory.
- While `check` inspects registered symlinks, `scan` can also find unregistered ones. Use `adopt` to register a symlink you find. Registering a symlink to a directory does not register symlinks inside that directory.

## Output

The default `human` format groups `scan` results into registered and unregistered symlinks, with problems first in each group. When `check` finds no problems, it prints `OK N links`.

In human output, path fields use double quotes and JSON escaping for link locations, targets, and parent directories. Absolute paths are shown without home-directory abbreviations; relative targets remain relative. Display cleanup removes interior `/./` components and collapses repeated separators, while preserving `..`, a leading `./`, and the number of leading `/` characters. A trailing `/` or `/.` is retained; these two endings are not combined. Repeated trailing slashes become one `/`.

This formatting only affects output. It does not change stored paths, symlinks, or the acceptance of `~/` in command arguments. Literal `~` characters in names remain literal. `--config` still prints an unquoted absolute path for use in command substitution.

To disable colors, set the `NO_COLOR` environment variable or use `TERM=dumb`.

### Changes and previews

Creating, registering, unregistering, removing, or restoring a symlink displays its location and the result, followed by the registered target on the next line.

`fix` displays changed symlinks and symlinks with target problems, and summarizes healthy unchanged symlinks by count. For example, restoring one missing symlink while leaving 22 healthy symlinks unchanged produces:

```text
✓ "/Users/you/links/example.txt" — created
  → "/Users/you/files/example.txt"

1 changed, 22 unchanged
```

Running `slink fix -n` previews the same operation without writing:

```text
○ "/Users/you/links/example.txt" — would create
  → "/Users/you/files/example.txt"

1 change planned, 22 unchanged
```

- `fix -n` predicts whether targets will be reachable after all selected symlinks have been restored.
- `✓` marks a completed operation without a target warning, `○` marks a preview, and `!` marks a target warning or a failed operation.
- `changed` counts entries whose symlink or registration changed; `unchanged` counts entries that needed no change. Failures are counted when present. Target problems are counted separately from failures to create or restore symlinks.

### TSV

Use `-o tsv` to process output in scripts:

```sh
slink list -o tsv
slink check -o tsv
slink scan -o tsv
```

Output has a header and one row per symlink.

| Command | Columns in order |
| --- | --- |
| `list` | `LINK`, `TARGET` |
| `check` | `LINK`, `LINK_STATE`, `TARGET`, `TARGET_STATE`, `ACTUAL_TARGET`, `ACTUAL_TARGET_STATE` |
| `scan` | `MANAGEMENT`, `LINK`, `LINK_STATE`, `TARGET`, `TARGET_STATE`, `ACTUAL_TARGET`, `ACTUAL_TARGET_STATE` |

| Column | Contents |
| --- | --- |
| `MANAGEMENT` | Whether the symlink is registered |
| `LINK` | The symlink's location |
| `LINK_STATE` | Whether the symlink matches its registration |
| `TARGET` | The registered target: stored text in `list`, validated and normalized absolute path in `check` and registered `scan` rows |
| `TARGET_STATE` | Whether the registered target is reachable |
| `ACTUAL_TARGET` | The target path read from the actual symlink; it may be relative |
| `ACTUAL_TARGET_STATE` | Whether the actual symlink's target is reachable |

- Path cells are JSON strings, and absent values are empty cells. After JSON decoding, `list`'s `LINK` and `TARGET` retain the stored strings exactly, and `ACTUAL_TARGET` retains the string read from the symlink, including relative paths, `/./`, repeated separators, and directory-only endings.
- `check` and `scan` use the display cleanup described above for `LINK` and the registered `TARGET`. A matching symlink can have different text in `TARGET` and `ACTUAL_TARGET`, for example after adopting a symlink that uses a relative path. Use the state columns for the diagnosis rather than comparing displayed strings.
- For unregistered `scan` rows, `LINK_STATE`, `TARGET`, and `TARGET_STATE` are empty, and `ACTUAL_TARGET` shows the actual target path.
- Results go to standard output (stdout), and error reasons go to standard error (stderr).
- Standalone paths in `ERROR` records and both paths in `PENDING` notices use the same display cleanup and quoting. Error reasons remain text and are not cleaned as paths.

## Limitations and recovery

- Even with `-f`, regular files and directories cannot be replaced with symlinks. A symlink that points directly to itself cannot be created or restored.
- If `~/B` is a symlink to a directory, you cannot manage both `~/B` and symlinks inside it, such as `~/B/C`, at the same time. Choose either to manage `~/B` itself or to manage the symlinks inside it individually.
- A symlink cannot occupy the location of the registry file or its lock or recovery files. Directories containing those files also cannot be managed as symlinks. For example, if you use `~/.config/slink/links.toml`, you cannot manage `~/.config/slink` itself. Paths must be representable in UTF-8.
- If creation, removal, or replacement is interrupted, repeat the same command with the same arguments and options to recover. Use `check` to see whether an operation is incomplete. Other changes are blocked until recovery completes.
- Keep any lock or recovery files next to the registry (such as `links.toml.slink-lock` and `links.toml.slink-pending`) and temporary `.slink-*` recovery directories until recovery is complete.
- When processing multiple symlinks, a failure does not undo changes that have already completed.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | The operation completed; for `check`, all selected symlinks are healthy |
| `1` | `check` found a problem, an operation failed or conflicted, or `scan` could not complete an inspection |
| `2` | Invalid arguments or registry, an unavailable registry, blocked recovery, or another command error |

- `list` returns `0` when it can read and display registrations. Use `check` to find out whether paths and symlinks are valid.
- Successful creation or `fix` returns `0` even if the target does not exist. `check` returns `1` for problems with symlinks or their targets, and `2` for invalid registry values.
- `scan` returns `1` for permission or I/O errors, but does not fail merely because it finds symlinks whose targets are missing. Trying to fix a symlink with a different target without `-f` returns `1`.

## Development documentation

- [Internal design](slink-design.md) (Japanese): responsibilities, path comparisons, change planning, and recovery.
- [Default behavior](docs/defaults.md): defaults and their rationale.
- [Path display design](docs/path-display.md) (Japanese): where to share formatting and where to preserve raw strings.
- [Homebrew releases](docs/homebrew.md): release and tap maintenance.
