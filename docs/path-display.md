# Path display design

This document defines how slink formats paths and where that formatting belongs.
It describes the implemented behavior and the requirements that future changes must preserve.
For the user-facing output contract, see the [README](../README.md#output).
For path interpretation, comparison, and persistence, see the [internal design](../slink-design.md).

## Choose the formatter by the value's purpose

A path shown for readability and a string returned for exact retrieval have different requirements.
Human path fields and some TSV fields use display formatting; TSV fields that expose stored or observed strings require exact preservation.

| Helper | Purpose |
| --- | --- |
| `display_path(path)` | Clean up the path's spelling for display, then add double quotes and JSON escaping. |
| `quoted(text)` | Add double quotes and JSON escaping while preserving the original string value. |

Display formatting never replaces a home-directory prefix with `~`, and it leaves relative paths relative.
CLI expansion of `~/` is a separate input feature; a `~` stored literally in a symlink target remains literal.

Human reason text uses `display_text()`; reasons in TSV `ERROR` records use `quoted()`.
Top-level errors keep their existing text rendering.
Formatting does not search completed error messages for path-like substrings and rewrite them.

## Where shared formatting applies

### Human output

All of the following path fields use `display_path()`.
Link locations, targets, and parent directories follow the same rules.

| Output | Path fields | Rendering entry point |
| --- | --- | --- |
| `list` | Link and target | Human branch of `list()` |
| `check` problem details | Link, target, expected target, and actual target | `render_diagnosis()` and `target_line()` |
| `check` pending operation | Link and target | Pending-operation block in `check()` |
| Healthy managed links in `scan` | Link and observed target | `print_scan_section()` |
| Managed links with problems in `scan` | Link, target, expected target, and actual target | Shared `render_diagnosis()` |
| Unmanaged links in `scan` | Link and observed target | `print_unmanaged()` |
| Scan traversal errors | Path that could not be scanned | `print_scan_human()` |
| Creation, `fix`, `adopt`, `remove`, and `unregister` | Link, target, and parent when present | `MutationOutput::print_result()` |
| Previews and recovery results for those operations | Link, target, and parent when present | The same `print_result()` |
| Operation failures | Standalone link field | `MutationOutput::failure()` |
| Failures caused by a target mismatch | Link, expected target, and actual target | `failure()` and the shared `target_line()` |

`push_target()` combines `target_line()` with a health annotation.
Command handlers in the engine pass values to the output layer; they do not format paths themselves.
The choice of which target to show, such as the observed target for a healthy scan result, is independent of how that value is formatted.

Inspection and comparison keep using the original path data, including when they happen inside an output module.
In particular, display cleanup does not belong in `collect_scan()`, `observed_health()`, `projected_health()`, or `projected_key()`.

### TSV output and its stderr diagnostics

Some TSV fields are formatted paths; others expose exact stored or observed strings.
The following table defines the boundary.

| Output | Field | Helper | Value represented |
| --- | --- | --- | --- |
| `list` stdout | LINK and TARGET | `quoted()` | Original registration strings read from TOML |
| `check` stdout | LINK | `display_path()` | Validated link location |
| `check` stdout | TARGET | `display_path()` | Registered target after validation and path normalization |
| `check` stdout | ACTUAL_TARGET | `quoted()` | Exact readlink string, for both MATCH and MISMATCH |
| Managed `scan` stdout | LINK and TARGET | `display_path()` | Scanned link location and validated, normalized registered target |
| Managed `scan` stdout | ACTUAL_TARGET | `quoted()` | Exact readlink string |
| Unmanaged `scan` stdout | LINK | `display_path()` | Scanned link location |
| Unmanaged `scan` stdout | ACTUAL_TARGET | `quoted()` | Exact readlink string |
| Unmanaged `scan` stdout | TARGET, TARGET_STATE, and LINK_STATE | Empty cells | No registration exists |
| `check` / `scan` stderr: ERROR | Standalone link or path field | `display_path()` | Location associated with the error |
| The same ERROR records | Reason | `quoted()` | Explanatory text, with no path cleanup |
| `check` stderr: PENDING | Link and target | `display_path()` | Paths involved in the operation to recover |

`PENDING:` is a notice on stderr, not a TSV data row.
Both paths use `display_path()`; the operation name uses the `Operation` value's debug representation (`{:?}`).

Formatting preserves the established column names, column order, state codes, row order, and separation of stdout from stderr.
An absent value remains an empty cell. It must not become the quoted empty string `""`.
An empty string actually stored in a list entry is a value, however, and is emitted as JSON `""`.

Here, **original string** means the value obtained by parsing TOML or reading the symlink, not the TOML source's choice of quotes or escapes.
Preservation means `JSON decode(quoted(value)) == value`.
A field described as raw still uses quoting and escaping.

### Values that bypass path cleanup

| Value or operation | Treatment | Reason |
| --- | --- | --- |
| TSV fields marked `quoted()` above | Quote the original string | Scripts must be able to recover dots, repeated separators, and suffixes exactly. |
| Human reasons, hints, and headings | Existing text rendering | A complete sentence or label is not a path. |
| Top-level errors in main and anyhow error chains | Existing error rendering | Completed messages are not rewritten in place. |
| Quoted invalid input and TOML error details | Existing input quoting | The original text helps identify the input error. |
| `--config` / `-c` | Unquoted absolute path | Supports command substitution, such as `open "$(slink --config)"`. |
| Input and registry validation, including `paths::normalize()` | Existing path operations | These operations interpret filesystem semantics. |
| Link matching, duplicate detection, sorting, and health checks | Original values and existing algorithms | Decisions must not depend on presentation. |
| Registry content, pending records, backups, and symlinks | Existing persistence operations | Display text is never written back as stored data. |

In an operation error, the standalone link field uses the shared formatter while the reason remains text.
If the reason also mentions that path, the two spellings are not required to match exactly.

## Display cleanup rules

Cleanup is a pure operation on the supplied text.
It does not read HOME or the working directory, inspect the filesystem, or depend on permissions or symlink state.

| Input feature | Display rule |
| --- | --- |
| Absolute path under the home directory | Keep the absolute form. |
| Relative path | Keep it relative. |
| Interior `.` component | Remove it. |
| Repeated `/` after the leading slash sequence | Collapse to one separator. |
| Trailing `/` | Retain a slash to preserve the directory requirement. |
| Trailing `/.` | Retain `/.`; do not turn it into `/`. |
| `..` components | Keep their positions and count, even directly below the root. |
| Leading `./` | Keep the initial relative `.` component. |
| Leading slash sequence | Keep its exact length, including `//` and `///`. |
| Empty string, standalone `.`, and root `/` | Keep the original value before quoting. |
| Dots within names, whitespace, and Unicode | Preserve them without trimming or Unicode normalization. |
| Backslash | Treat it as a Unix filename character, not a separator. |
| Quotes and control characters | Apply JSON escaping through `quoted()`, including the additional DEL and C1 escapes. |
| A Path containing non-UTF-8 bytes | Use the existing quoted placeholder `"<non-UTF-8>"` without a lossy conversion. |

Keeping a directory suffix does not imply preserving every character of its original spelling.
For example, `a///` and `a/./` both display as `"a/"`, while `a/.` displays as `"a/."`.
Fields that need every separator and dot use `quoted()` instead.

Leading slashes are preserved separately from separators elsewhere in the path.
A path consisting only of slashes therefore retains its original slash count.
The boundary cases `/./`, `/.`, and `./` display as `"/"`, `"/."`, and `"./"` respectively.
Restoring a trailing slash must not accidentally turn the root `/` into `//`.

Rebuilding a path with `Path::components().collect()` would discard trailing separators and `/.`.
See Rust's [Path::components documentation](https://doc.rust-lang.org/std/path/struct.Path.html#method.components).
Likewise, `paths::normalize()` is unsuitable for display because it consults the filesystem and can simplify `..`.

These preservation rules apply to the value received by the formatter.
The formatter does not reconstruct spelling already changed by input processing or registry validation.

### Examples

The output column includes the double quotes returned by `display_path()`.

| Input string | Formatted output |
| --- | --- |
| `/Users/example/.zshenv` | `"/Users/example/.zshenv"` |
| `/a/./b` | `"/a/b"` |
| `/a//./b` | `"/a/b"` |
| `/a/./b/` | `"/a/b/"` |
| `/a/./b/.` | `"/a/b/."` |
| `/a/b///` | `"/a/b/"` |
| `/a/b/./` | `"/a/b/"` |
| `/a/../b` | `"/a/../b"` |
| `/../b` | `"/../b"` |
| `../a/./b` | `"../a/b"` |
| `./a/./b` | `"./a/b"` |
| `~/a/./b` (literal `~`) | `"~/a/b"` |
| `//server//a/./b/.` | `"//server/a/b/."` |
| `///a//b` | `"///a/b"` |
| `/`, `//`, `///` | `"/"`, `"//"`, `"///"` |
| `/.`, `/./` | `"/."`, `"/"` |
| `.`, `./`, `././` | `"."`, `"./"`, `"./"` |
| Empty string | `""` |
| ` a ` | `" a "` |

For example, the observed target `../a/./b` appears as `"../a/b"` in human output and as `"../a/./b"` in TSV ACTUAL_TARGET.
Both use the same quoting; only the human path is cleaned up.

## Helper responsibilities

The path display helpers are internal to [src/output.rs](../src/output.rs) and its `mutation` child module.
They are separate from [src/paths.rs](../src/paths.rs), which interprets and compares paths.
This separation keeps formatted output from becoming an input to filesystem operations.

| Helper | Responsibility |
| --- | --- |
| `clean_display_path(text: &str) -> String` | Apply the cleanup rules to the spelling only, without quoting. |
| `display_path(path: impl AsRef<Path>) -> String` | Accept Path, PathBuf, str, or String values; obtain text, clean it up, and quote it. |
| `quoted(text: &str) -> String` | Apply JSON quoting and control-character escaping without interpreting the value. |
| `display_text(text: &str) -> String` | Escape human-readable explanatory text. |
| `target_line()` / `push_target()` | Combine shared path formatting with labels and health annotations. |
| `render_diagnosis()` / `MutationOutput` | Assemble diagnostic and operation-result layouts. |

The common path formatter is:

```rust
fn display_path(path: impl AsRef<Path>) -> String {
    let text = path.as_ref().to_str().unwrap_or("<non-UTF-8>");
    quoted(&clean_display_path(text))
}
```

`clean_display_path()` splits the original UTF-8 string on `/`.
It identifies leading slashes, an initial relative `.`, and trailing `/` or `/.` from that string.
It removes interior empty and dot components, leaving `..` and ordinary names in order.
Returning a String avoids losing suffix information through path reconstruction.

Call sites choose between `display_path()` and `quoted()` according to the field's purpose.
There is no output-format argument or `raw: bool` switch.
Both helpers share the same quoting code. `quoted()` must remain independent of path cleanup because reasons and raw TSV values also use it.
There is no separate cleanup algorithm for TSV or separate wrapper for link and target fields.

## Implementation and test coverage

| File | Implementation or coverage |
| --- | --- |
| [src/output.rs](../src/output.rs) | Shared helpers; human path fields; applicable TSV fields; ERROR and PENDING paths; unit tests for cleanup boundaries. |
| [src/output/mutation.rs](../src/output/mutation.rs) | Link, target, parent, and failure-path rendering; expected and actual targets through `target_line()`. |
| [tests/read_output.rs](../tests/read_output.rs) | List and scan output; the boundary between formatted paths and raw check/scan fields; ERROR path fields. |
| [tests/check_output.rs](../tests/check_output.rs) | Quoting, control characters, absolute paths without home abbreviation, mismatches, and pending operations in human output. |
| [tests/check_format.rs](../tests/check_format.rs) | Fixed TSV columns, TARGET versus ACTUAL_TARGET, PENDING quoting, and preservation of recovery data. |
| [tests/mutation_output.rs](../tests/mutation_output.rs) | Mutations, previews, recovery, parent paths, and failures. |
| [tests/registry_reading.rs](../tests/registry_reading.rs) | Exact TSV list values and the separation between listing invalid entries and validating entries for mutation. |
| [tests/redesign.rs](../tests/redesign.rs) | Storage, display, and diagnosis of `file/` and `file/.`; CLI `~/` expansion, literal readlink `~`, and path semantics. |
| [README.md](../README.md#output) and [README.ja.md](../README.ja.md#出力) | The user-facing human and TSV output contract, with examples. |

## Acceptance criteria

| Area | Required behavior |
| --- | --- |
| Cleanup rules | Verify the examples above and boundary cases including `a/./.`, `a/.//`, `./.`, and `//./`. |
| Idempotence | `clean(clean(s)) == clean(s)`. This applies to cleanup, not to feeding quoted output back into the formatter. |
| Quoting and controls | JSON decoding recovers the expected text for paths containing spaces, quotes, backslashes, newlines, tabs, DEL, C1, and Japanese characters. Control characters cannot add output rows or columns. |
| Non-UTF-8 paths | Formatting returns the quoted placeholder without panicking. |
| Consistent human paths | The same input has the same formatting as a link, target, or parent. Home-directory paths do not acquire a generated `~`. |
| List values | TSV values decode to the exact stored strings, including interior dots, repeated slashes, suffixes, empty strings, and literal `~`. Human output applies cleanup. |
| Check and scan values | ACTUAL_TARGET decodes to the exact readlink string, including forms such as `../a//./b/.`. LINK and registered TARGET follow the display rules. |
| Unmanaged scan rows | The three registration-related cells remain empty, and ACTUAL_TARGET retains the observed string. |
| ERROR and PENDING | Only path fields receive cleanup. Reasons retain their text after decoding, and both PENDING paths use the same formatter. |
| Mutation output | Normal operations, previews, recovery, parent creation, TargetMismatch errors, and other failures use the shared path formatting. |
| Diagnostic meaning | Formatting does not change diagnoses or exit codes for `file`, `file/`, `file/.`, or paths containing `..` after a symlink. Decisions use the original data. |
| Persistent state | List, check, scan, and dry-run leave the registry, existing symlinks, and pending records unchanged. Executed mutations retain their persistence and recovery guarantees. |
| Other contracts | Preserve the unquoted absolute `--config` output, CLI `~/` expansion, literal `~`, and information in ordinary error causes. |

A visual match between displayed paths is never evidence for recalculating MATCH or target health.
Tests of exact TSV values compare JSON-decoded strings; Path equality can hide differences in spelling.

Existing [config tests](../tests/config.rs), [path and command tests](../tests/redesign.rs), [CLI tests](../tests/cli.rs), and recovery tests provide regression coverage.
[CI](../.github/workflows/ci.yml) runs formatting checks, clippy, tests, release builds, and APFS checks on macOS to verify both presentation and path behavior.
