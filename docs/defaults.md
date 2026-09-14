# Default behavior

| Behavior | Default | Reason |
| --- | --- | --- |
| Interpret CLI relative paths | Working directory | Both operands use one starting point. |
| Accept leading `~/` | CLI input only | Input can be short while storage stays absolute. |
| Store registry paths | Absolute only, schema v2 | Hand edits and CLI writes follow the same rule. |
| Create or restore symlinks | Absolute target | No separate relative-generation mode or saved option is needed. |
| Register a matching existing symlink | On | Creation can bring registrations into agreement without a separate adopt step. |
| Replace a different symlink | Off; `-f` / `--force` | Existing references can be intentional. Both create and fix use the same rule. |
| Create link parent directories | Off; `-p` / `--parents` | A mistyped parent is reported instead of silently created. |
| Simulate mutations | Off; `-n` / `--dry-run` | Mutation commands normally perform the requested operation. |
| Keep a removed link | Off; `remove -k` / `--keep-link` | Remove normally deletes the managed link and unregisters it. |
| Scan subdirectories | Off; `-R` / `--recursive` | The default scope is the selected directory's immediate children. |
| Follow directory symlinks during scan | Never | A directory link is an entry to report, not a subtree to traverse. |
| Preserve unrelated TOML values, comments and ordering | On | CLI edits coexist with hand edits. |
| Allow missing targets and report their state | On | Targets can be created or mounted later; check diagnoses availability. |
| Protect ordinary files and directories | On, including with force | Replacing a symlink does not authorize deleting other data. |

A registry entry contains only an absolute link location and absolute reference target. Options apply to the current invocation and are not saved. `adopt` preserves existing symlinks, including relative target text, while recording an absolute reference. Matching uses that reference; a later restoration uses an absolute target.

Paths containing symlinks are not replaced with their final destinations. Meaningful `..` components and target directory suffixes are preserved. See the [path model](../README.md#paths) for examples.

Creation, fix, and adopt share a plan of filesystem and registry changes. Their authority differs: CLI arguments, registry entries, and existing symlinks respectively. Dry-run uses the same plan without filesystem writes.
