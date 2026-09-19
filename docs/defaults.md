# Default behavior

| Behavior | Default | Reason |
| --- | --- | --- |
| Interpret CLI relative paths | Working directory | Both operands use one starting point. |
| Accept leading `~/` | CLI input only | Input can be short while storage stays absolute. |
| Store registry paths | Absolute only | Hand edits and CLI writes follow the same rule. |
| List registrations | Read all entries without path validation; format paths in human output | Hand-edited values stay visible, including mistakes; `list -o tsv` preserves their stored strings exactly. |
| Format displayed paths | Double quotes and JSON escaping, without home abbreviations | Link locations, targets, and parent paths use the same display rules; relative targets stay relative. |
| Preserve raw TSV values | `list` LINK/TARGET and all ACTUAL_TARGET cells | Scripts can retrieve the stored or observed strings after JSON decoding. Other path fields use display cleanup. |
| Print the registry location with `--config` | Unquoted absolute path | The value can be used directly in command substitution. |
| Display mutation results | One human-readable block per reported link | Paths, targets, and diagnostics share the read-only commands' display rules. |
| Display healthy unchanged links during fix | Count only | Changed links and target problems stay visible. |
| Create or restore symlinks | Absolute target | New links use one target representation. |
| Register a matching existing symlink | On | Creation can bring registrations into agreement without a separate adopt step. |
| Replace a different symlink | Off; `-f` / `--force` | Existing references can be intentional. Both create and fix use the same rule. |
| Create link parent directories | Off; `-p` / `--parents` | A mistyped parent is reported instead of silently created. |
| Simulate mutations | Off; `-n` / `--dry-run` | Mutation commands normally perform the requested operation. |
| Unregister a symlink | Keep the symlink | `unregister` changes only the registry; use `remove` to delete the symlink as well. |
| Scan subdirectories | Off; `-R` / `--recursive` | The default scope is the selected directory's immediate children. |
| Follow directory symlinks during scan | Never | A directory link is an entry to report, not a subtree to traverse. |
| Preserve unrelated TOML values, comments and ordering | On | CLI edits coexist with hand edits. |
| Allow missing targets and report their state | On | Targets can be created or mounted later; check diagnoses availability. |
| Protect ordinary files and directories | On, including with force | Replacing a symlink does not authorize deleting other data. |

A registry entry contains only an absolute link location and absolute reference target. Options apply to the current invocation and are not saved. `adopt` preserves existing symlinks, including relative target text, while recording an absolute reference. Matching uses that reference; a later restoration uses an absolute target.

Paths containing symlinks are not replaced with their final destinations. Meaningful `..` components and target directory suffixes are preserved. See the [path model](../README.md#paths) for examples.

Display cleanup removes interior `/./` and repeated separators without changing stored data or path comparisons. It preserves `..` and distinguishes trailing `/` from `/.`. See the [output contract](../README.md#output) and [path display design](path-display.md) for the exact scope, including TSV and diagnostic fields.

Creation, fix, and adopt share a plan of filesystem and registry changes. Their authority differs: CLI arguments, registry entries, and existing symlinks respectively. Dry-run uses the same plan without filesystem writes.
