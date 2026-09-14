# Default behavior

`--relative` and `--parents` remain opt-in. Both can be made defaults, but neither is unconditionally the right behavior.

| Behavior | Default | Reason |
| --- | --- | --- |
| Generate relative targets | Off; `--relative` | Relative links survive moving a source/link tree together, but moving just the link can break them. The default preserves the target operand literally, like `ln -s`. |
| Create link parent directories | Off; `--parents` | A missing parent often indicates a mistyped destination. Automatic directory creation can hide that mistake. |
| Replace mismatched links | Off; `fix --replace` | A different target can be an intentional manual change. |
| Simulate mutations | Off; `--dry-run` | A create/remove command should perform its named operation. |
| Keep a removed link | Off; `remove --keep-link` | The normal meaning of remove is deleting the managed link and unregistering it. |
| Register newly created links | On | This is the defining behavior of slink. |
| Preserve TOML comments and ordering | On | Editing through the CLI should coexist with hand editing. |
| Allow missing targets and report their state | On | Targets may be created or mounted later; `check` diagnoses their availability. |
| Protect ordinary files and directories | On | No generic force option replaces user data. |
| Scan recursively without following symlink directories | On | Discovery stays inside the selected directory tree. |

Without `--relative`, `slink src sub/link` stores `src`, which resolves to `sub/src`.
With `--relative`, the same operands produce `../src`, referring to `src` in the working directory (the directory where the command runs).
If the supplied target path names or passes through another symbolic link, conversion keeps that reference. It also preserves `..` components whose removal would change which path is reached. See the [path model](../README.md#path-model) and [`--relative` examples](../README.md#options-and-defaults).

`--parents` creates only link parent directories, never target directories. Removing a link leaves those parents in place. Recreating parents later requires `fix --parents` again.

No `relative` or `parents` settings are stored in the registry file. Each entry stores only the link path and the target text. `--relative` determines that text; `--parents` applies only to the current command.
