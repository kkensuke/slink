# slink internal design

slink manages two related states: the symlinks on disk and their registrations in a TOML file.
Each command chooses which information drives an update. Path interpretation, inspection, planning, persistence, and presentation have separate responsibilities.

This document explains those responsibilities and the guarantees they provide.
See the [README](README.md) for commands, options, and exit codes; [Default behavior](docs/defaults.md) for the reasons behind the defaults; and [Path display design](docs/path-display.md) for formatting rules and their scope.

## Module responsibilities

| Module | Responsibility |
| --- | --- |
| [main.rs](src/main.rs) and [cli.rs](src/cli.rs) | Parse and validate arguments, enter command execution, and handle top-level errors and exit codes. |
| [engine.rs](src/engine.rs) | Select entries, build change plans from observations, and coordinate execution and recovery. |
| [paths.rs](src/paths.rs) | Interpret input paths, normalize references for comparison, and identify link locations. |
| [registry.rs](src/registry.rs) | Read and validate TOML, edit registrations, lock the registry, detect concurrent edits, and save atomically. |
| [inspect.rs](src/inspect.rs) | Capture filesystem snapshots and diagnose whether a symlink matches its registration and whether its targets are reachable. |
| [transaction.rs](src/transaction.rs) | Record pending operations, back up existing symlinks, and execute or resume changes. |
| [output.rs](src/output.rs) | Render list, check, and scan output; collect and order scan results; provide shared display formatting. |
| [output/mutation.rs](src/output/mutation.rs) | Report changes, previews, and recovery results; count outcomes; evaluate target health after a fix batch. |

Some filesystem inspection and health evaluation currently live in the output modules.
Those operations still use the original path data. A function's location in an output module does not make display formatting appropriate for its inputs.

## What drives each operation

Commands share planning and execution machinery, while choosing different sources of information for the desired state.

| Operation | Basis for the change | State updated |
| --- | --- | --- |
| `slink <target> <link>` | CLI arguments | Symlink and registry |
| `slink fix [link ...]` | Registry entries | Symlinks |
| `slink adopt <link ...>` | Existing symlinks | Registry |
| `slink unregister <link ...>` | Selected registry entries | Registry only |
| `slink remove <link ...>` | A registered symlink that matches its entry, or a registered location where the link is already absent | Symlink, if present, and registry |

Existing regular files and directories are protected from replacement and deletion.
If `remove` finds a symlink that differs from its registration, it fails and preserves both the symlink and the entry.
The README describes command arguments and conflict handling in detail.

## Path interpretation and comparison

Three values must remain distinct:

- **Link location:** the directory entry occupied by the symlink.
- **Registered target:** the absolute reference path stored in the registry.
- **Observed target:** the string returned by readlink, which may be relative and may use a different spelling from the registered target.

The `paths` module converts CLI input to absolute paths. Relative input uses the working directory, and `~` or a leading `~/` uses the home directory.
Registry values must already be absolute in both fields; reading the registry does not expand `~`.

A target reference keeps the symlinks named along its path.
`paths::normalize()` consults the filesystem before collapsing a parent component: it can simplify `ordinary-directory/..`, but preserves `..` after a symlink or a missing or inaccessible component.
It also preserves target suffixes that require a directory, such as `/` and `/.`.
Link identity uses the containing directory and the final name without following the link itself, with APFS case sensitivity and Unicode equivalence taken into account.

`adopt` leaves the existing symlink untouched.
It converts a relative observed target to an absolute reference using the directory containing the link, treating any `~` returned by readlink as a literal filename character.
`check`, `fix`, and `remove` use the same reference conversion when comparing targets.
Two different chains of symlinks are not considered equivalent merely because they ultimately reach the same object.

New and restored symlinks use absolute targets.
As a result, `fix` can restore the registered reference without reproducing the original relative target string.

Display formatting is a separate operation. `display_path()` prepares text for output; its result is never used for path comparison, persistence, or filesystem operations.

## Reading, validating, and saving the registry

The registry is a TOML document intended to support manual editing.
Each `[[link]]` table contains `link` and `target` strings; an empty file represents zero registrations.
Parsing and path validation are separate so that users can inspect entries that need repair.

- `list` uses `Registry::read_entries()` to read stored strings in file order. It can show invalid paths and duplicate registrations without inspecting the links or their targets.
- `check`, `scan`, and mutation commands use `Registry::open()` to validate paths and detect duplicates across the entire registry.
- Invalid TOML syntax or entry structure prevents even `list` from reading the file. No partial list is printed, and invalid registry paths are never repaired by guessing from the working directory.

Human list output formats both stored path strings for readability.
TSV list output preserves their values exactly after JSON decoding, so scripts can retrieve what was registered.
Neither output format writes display text back to the registry.

When the registry path is itself a symlink, updates go to its referent.
New entries align `link   =` with `target =`; edits preserve existing whitespace, comments, ordering, and line endings around unchanged content.
Writes use locking, concurrent-edit detection, and atomic replacement.

## Planning and target health

Observation, planning, and execution are separate steps. Dry-run and execution consume the same change plan.

Within a selected `fix` batch, dependencies between managed symlinks are processed before their dependents.
Ordering is deterministic even for independent entries or dependency cycles, so reordering the registry alone does not change results or warnings.
Explicit link arguments define the set to process; dependencies outside that set are not added automatically.

Target health describes whether a target is reachable.
For `fix`, warnings reflect the filesystem after the selected batch has been processed, rather than the intermediate state after each individual change.
For `fix -n`, health is evaluated against a projected state that includes the planned symlinks.

Projection also handles a planned symlink encountered partway through a target path.
It combines that symlink's target with the remaining path components and evaluates meaningful `..` components at the same point that ordinary filesystem traversal would.

## Transactions and recovery

Before replacement or removal, slink verifies the existing symlink and moves it to a backup.
A pending-operation record allows an interrupted change to resume when the user repeats the same operation.
Recovery checks the request and the current filesystem state. If either conflicts with the recorded operation, recovery stops and retains the record.

Mutation safeguards also reject link locations that collide with the registry or its internal support files.
Processing multiple links is not one atomic transaction: changes that have already completed remain in place if another item fails.

## Reporting and verification

Human and TSV output use the same diagnoses.
Formatting never recomputes target matches or health from the displayed strings.
Human path fields and selected TSV fields use `display_path()`; TSV fields that expose original strings use `quoted()`.
The [path display design](docs/path-display.md) defines those fields, the treatment of reasons and `--config`, and the distinction between trailing `/` and `/.`.

Mutation commands pass results to `MutationOutput` after the planned changes complete.
Previews use labels and markers that identify the work as proposed, and completed recovery uses the same result layout as other changes.
For `fix`, healthy unchanged entries are summarized by count, while changes and target problems are shown individually.

Changed, unchanged, and failed operations are counted separately from target problems.
An incomplete or failed operation is never reported as successful; operation errors include the affected location and reason on stderr.

The [tests](tests) cover CLI combinations, path semantics, registrations, mutations, scans, manual edits, dry-run, and recovery from interrupted operations.
[CI](.github/workflows/ci.yml) runs formatting checks, clippy, tests, and release builds; macOS jobs also exercise both APFS case-sensitivity modes.
The English and Japanese READMEs describe the same examples and behavior. Release procedures are documented in [Homebrew releases](docs/homebrew.md).
