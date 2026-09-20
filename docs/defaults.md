# Default behavior

| Behavior | Default | Reason |
| --- | --- | --- |
| Interpret CLI relative paths | Working directory | Both operands use one starting point. |
| Accept leading `~/` | CLI input only | Input can be short while storage stays absolute. |
| Store registry paths | Absolute `link`; canonical absolute or relative `target` | The target text is the representation slink will materialize. |
| List registrations | Read all entries without path validation; format paths in human output | Hand-edited values stay visible, including mistakes; `list -o tsv` preserves their stored strings exactly. |
| Format displayed paths | Double quotes and JSON escaping, without home abbreviations | Link locations, targets, and parent paths use the same display rules; relative targets stay relative. |
| Preserve raw TSV values | `list` LINK/TARGET and all ACTUAL_TARGET cells | Scripts can retrieve the stored or observed strings after JSON decoding. Other path fields use display cleanup. |
| Print the registry location with `--config` | Unquoted absolute path | The value can be used directly in command substitution. |
| Display mutation results | One human-readable block per reported link | Paths, targets, and diagnostics share the read-only commands' display rules. |
| Display healthy unchanged links during fix | Count only | Changed links and target problems stay visible. |
| Create symlinks | Absolute target by default; `-r` / `--relative` selects relative | CLI operands keep one cwd-based interpretation while target representation is explicit. |
| Restore symlinks | Registered target representation | `fix` recreates the target text already stored in the registry. |
| Register a matching existing symlink | On | Creation can bring registrations into agreement without a separate adopt step. |
| Replace or re-represent a symlink | Off; `-f` / `--force` | Without force, semantically correct links are kept; with force, create/fix may enforce the requested or registered canonical target text. |
| Create link parent directories | Off; `-p` / `--parents` | A mistyped parent is reported instead of silently created. |
| Simulate mutations | Off; `-n` / `--dry-run` | Mutation commands normally perform the requested operation. |
| Unregister a symlink | Keep the symlink | `unregister` changes only the registry; use `remove` to delete the symlink as well. |
| Scan subdirectories | Off; `-R` / `--recursive` | The default scope is the selected directory's immediate children. |
| Follow directory symlinks during scan | Never | A directory link is an entry to report, not a subtree to traverse. |
| Preserve unrelated TOML values, comments and ordering | On | CLI edits coexist with hand edits. |
| Allow missing targets and report their state | On | Targets can be created or mounted later; check diagnoses availability. |
| Protect ordinary files and directories | On, including with force | Replacing a symlink does not authorize deleting other data. |

A registry entry contains only an absolute link location and canonical target text. The target may be absolute or relative; options are not saved separately because the target text already records the representation. `adopt` leaves the existing symlink untouched and stores a canonical form of its observed target text. A later restoration writes that registered representation directly.

Canonical target spelling removes only semantically irrelevant syntax such as interior `.`, redundant ordinary separators, a redundant leading `./`, and trailing `/.` in favor of trailing `/`. Meaningful `..` components, absolute versus relative form, directory requirements, and symlink-sensitive references are preserved. Semantic matching applies the same canonical spelling before link-aware reference comparison.

Display cleanup is separate from persistence canonicalization. It affects only presentation and can still clean externally observed target text without changing the filesystem. See the [output contract](../README.md#output) and [path display design](path-display.md) for the exact scope, including TSV and diagnostic fields.

Creation, fix, and adopt share a plan of filesystem and registry changes. Their authority differs: CLI arguments, registry entries, and existing symlinks respectively. Dry-run uses the same plan without filesystem writes.
