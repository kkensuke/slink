# Relative symlink target design

## Status

Design proposal. Implementation is intentionally out of scope for this pull request.

## Goal

Add first-class support for creating, adopting, checking, restoring, and safely recovering relative symlinks without adding a separate representation policy to the registry.

The design should preserve the current path semantics wherever possible and keep both the implementation and the user model small.

## Non-goals

This proposal does not make registry entries relocatable as a whole.

In particular, `link` remains an absolute path. Moving a project tree can therefore preserve a relative symlink on disk while leaving the registry pointing at the old link location.

A separate design would be required if the goal becomes:

- moving or cloning a project and using the same registry unchanged;
- sharing one registry across machines with different absolute paths; or
- storing `link` itself relative to another base.

This proposal is limited to preserving the target representation of managed symlinks.

## Core model

A registry entry contains the two values needed to reconstruct a symlink:

```toml
[[link]]
link   = "/Users/me/project/links/config"
target = "../config"
```

The fields mean:

| Field | Meaning |
| --- | --- |
| `link` | Absolute location of the managed symlink |
| `target` | Exact target text to write into the symlink |

The target may be absolute or relative.

A relative target is interpreted by the operating system from the directory containing the symlink. It is not relative to the registry file and is not relative to the process working directory.

The semantic reference of an entry is therefore derived from the pair:

```text
(link, target)
```

Conceptually:

```text
reference = reference_target(link, target)
```

The `target` value alone does not identify a reference when it is relative.

This model deliberately avoids adding fields such as:

```toml
relative = true
style = "relative"
```

or storing both an absolute reference and a materialized target.

The target text already contains the filesystem representation that `fix` must restore.

## Design invariants

The implementation should preserve these invariants:

1. `link` identifies where the managed symlink lives and remains absolute.
2. `target` is the exact text slink intends to write with `symlink(2)`.
3. Any code that needs the target's path meaning derives it from `(link, target)`; it must not assume that `target` is absolute.
4. Steady-state correctness uses semantic reference comparison.
5. Transaction recovery uses exact target-text comparison when deciding whether filesystem state belongs to the pending operation.

These rules keep representation persistence separate from reference comparison without adding another persisted policy.

## Why store target text directly?

The alternative model is to store an absolute semantic reference and separately remember whether the symlink should be materialized as absolute or relative.

For example:

```toml
link     = "/Users/me/project/links/config"
target   = "/Users/me/project/config"
relative = true
```

That model is valid, but it stores two pieces of state in order to reconstruct one symlink target.

Storing the actual symlink target text instead gives the same restoration information with the existing two-field schema:

```toml
link   = "/Users/me/project/links/config"
target = "../config"
```

The restoration path stays simple:

```rust
symlink(&entry.target, &link)
```

No representation field, materialization layer, or target-style policy is required during `fix` or transaction recovery.

## Creation

A new create-only option is added:

```text
--relative
```

Example:

```sh
cd /Users/me/project
slink --relative -p config links/config
```

The result is:

```text
links/config -> ../config
```

and the registry stores:

```toml
[[link]]
link   = "/Users/me/project/links/config"
target = "../config"
```

### CLI target interpretation does not change

Both operands keep the current CLI meaning: relative command-line paths are interpreted from the working directory.

For `--relative`, the target is not interpreted from the link parent directly.

Instead:

```text
CLI target
    |
    | interpret exactly as today, relative to cwd
    v
absolute reference
    |
    | encode relative to the actual containing directory of link
    v
relative target text
    |
    +--> registry target
    +--> symlink target
```

For example:

```sh
cd /Users/me/project
slink --relative config links/config
```

first interprets `config` as:

```text
/Users/me/project/config
```

and then encodes that reference from the directory containing `links/config`, producing:

```text
../config
```

This preserves the existing mental model for CLI operands.

## Relative target generation

Relative target generation must preserve the existing path semantics instead of applying unconditional lexical simplification.

In particular, it must not canonicalize the target reference before generating the relative form.

The current implementation intentionally distinguishes cases involving:

- symlinks in parent paths;
- meaningful `..` components;
- missing path components;
- target paths that themselves pass through symlinks;
- trailing `/` and `/.`.

The relative conversion should therefore use the same notion of the link's actual containing directory that current path handling already uses.

Conceptually:

```text
base = directory_location(link.parent())
candidate = make_relative(base, absolute_reference)
```

### Mandatory round-trip verification

A generated relative candidate is accepted only if it preserves the same reference under the existing path semantics:

```rust
reference_target(link, candidate) == original_absolute_reference
```

If that condition cannot be satisfied safely, creation fails rather than emitting a target with changed semantics.

This lets the existing `reference_target()` behavior remain the authority for path interpretation instead of reimplementing all of its rules inside the relative-path generator.

## Adoption

`adopt` should preserve the target text observed with `readlink()`.

Given:

```text
links/config -> ../config
```

then:

```sh
slink adopt links/config
```

stores:

```toml
[[link]]
link   = "/absolute/path/to/links/config"
target = "../config"
```

It does not convert the observed target to an absolute string.

This gives the desired invariant:

```text
adopt
  -> store observed target text

delete link

fix
  -> restore the same target text
```

No `--relative` option is needed for `adopt`.

## Checking and semantic equality

Steady-state comparison should continue to compare references rather than raw target strings.

Conceptually:

```rust
target_matches(link, actual_target, registered_target)
```

remains:

```text
reference_target(link, actual_target)
    ==
reference_target(link, registered_target)
```

Therefore an existing absolute and relative symlink representation may both be considered correct when they express the same reference.

Example:

```text
registry:
    target = "../config"

filesystem:
    link -> /Users/me/project/config
```

If both resolve to the same reference according to `reference_target()`, `check` reports a match.

This is intentionally reference equality, not final-inode equality. The design does not canonicalize away meaningful intermediate symlink references.

## Existing equivalent symlinks are not rewritten

Suppose an absolute symlink already exists:

```text
links/config -> /Users/me/project/config
```

and the user runs a relative creation command that refers to the same target.

If the existing symlink is semantically equivalent under `target_matches()`, it should be left untouched.

The registry may be updated to contain the relative target text, so a future missing-link restoration uses the newly requested representation.

This proposal does not turn `--relative` into a representation-conversion command.

Rewriting an already-correct symlink merely to change its textual representation would introduce a separate policy question around replacement, force behavior, inode changes, and transaction ownership.

## Fix

For an existing matching symlink, `fix` keeps the current semantic comparison behavior and leaves it untouched.

For a missing symlink, `fix` writes the stored target text directly:

```rust
symlink(&entry.target, &link)
```

Examples:

```toml
target = "/Users/me/project/config"
```

restores an absolute symlink target.

```toml
target = "../config"
```

restores a relative symlink target.

No `--relative` option is needed on `fix`; the registry already contains the representation to restore.

## Remove

`remove` continues to use semantic target comparison before deleting a managed symlink.

An equivalent absolute or relative target representation can therefore still match the registration.

No representation-specific removal behavior is required.

## Self-reference

Self-reference checks must interpret the registered target from the link location before comparing it with the link itself.

An absolute-target-only check is insufficient once registry targets may be relative.

Conceptually:

```text
resolved target reference = target_path(link, target)
```

and direct self-reference is rejected using that interpreted reference.

## Registry validation

The validation rules become asymmetric by design.

### `link`

`link` keeps the existing rules:

- must be absolute;
- must be a valid link path;
- continues to participate in duplicate/path-key validation.

### `target`

`target` is target text, not a registry location.

It therefore:

- may be absolute or relative;
- must be non-empty;
- must not contain NUL;
- must preserve its stored spelling rather than being normalized on registry load.

This is important because representation is now part of the persisted state.

## Transaction recovery

Transaction handling has two different equality requirements.

### Normal operation: semantic equality

During normal checking and planning, equivalent references are sufficient:

```text
reference equality
```

### Recovery ownership: exact target-text equality

During crash recovery, the transaction must distinguish the exact filesystem state it created from another semantically equivalent symlink that may have appeared independently.

Recovery therefore keeps exact target comparison:

```text
actual readlink() text == pending entry.target
```

This is stronger than normal `target_matches()` and is intentional.

Because the pending entry already contains the exact target text that should be created, no separate `materialized_target` field is necessary.

## Reconstructing a pending create request

One recovery path requires special care.

Today, create recovery can compare the command target derived from the repeated invocation with the target saved in the pending operation.

With `--relative`, these values differ unless the command target is passed through the same creation materialization step.

Example:

```text
CLI operand:
    config

cwd interpretation:
    /Users/me/project/config

pending target:
    ../config
```

Creation should therefore have one shared target-materialization function used by both:

- normal create planning; and
- repeated-command matching during pending recovery.

Conceptually:

```rust
materialize_create_target(link, target_operand, relative)
```

returns the exact target text that would be stored and written.

The recovery check compares this derived text with `pending.entry.target`.

This avoids adding a redundant representation field to `Pending` or `Request`.

## Fix dependency ordering

The existing dependency model can continue to derive a target path from the pair:

```text
(link, entry.target)
```

Relative registry targets therefore do not require a second dependency model.

Code that needs a path reference should use the existing link-aware target interpretation rather than assuming `entry.target` itself is absolute.

## Proposed implementation surface

The expected changes are intentionally narrow.

| Area | Change |
| --- | --- |
| `cli.rs` | Add create-only `--relative` |
| `paths.rs` | Add safe relative target generation, round-trip verification, and relative-aware self-reference handling |
| `registry.rs` | Keep `link` absolute; allow raw relative or absolute `target` text |
| `engine.rs` | Share create target materialization; make `adopt` store observed target text; use the same materialization for create recovery matching |
| `transaction.rs` | Stop requiring pending targets to be absolute; preserve direct creation and exact recovery comparison |

The major command semantics do not need separate relative-target branches for `check`, `remove`, or `fix`.

## Compatibility

Existing registries containing only absolute targets remain valid and keep their current behavior.

No schema migration is required.

A registry containing relative targets is not expected to work with older versions that require registry targets to be absolute.

That compatibility boundary should be documented when the feature is implemented.

## Tests

The implementation should cover at least the following cases:

1. `--relative` create stores and writes the same relative target text.
2. Deleting that symlink and running `fix` restores the same relative target text.
3. Adopting an existing relative symlink preserves its target text through delete and `fix`.
4. An equivalent absolute symlink matches a registered relative target without being rewritten.
5. Crash recovery uses exact target-text equality.
6. Repeated create after a crash derives the same materialized target as the pending operation.
7. Relative conversion preserves target references that pass through symlinks.
8. Meaningful `..` components retain their semantics.
9. Missing targets can still be represented safely.
10. Trailing `/` and `/.` behavior is preserved.
11. A symlink in the link's parent path is handled using the actual containing directory.
12. Direct self-reference is rejected for relative targets.
13. A generated candidate that fails round-trip verification is rejected.

Existing crash-recovery and dependency-order tests should be extended rather than replaced.

## Resulting user model

The complete model is:

```text
registry:
    link   = where the managed symlink lives
    target = what text belongs inside that symlink
```

Creation chooses the target representation once.

`adopt` observes it.

`fix` restores it.

`check` and `remove` compare what the target means.

Transaction recovery compares exactly what the transaction wrote.

That separation provides relative symlink support without introducing a second persistent representation policy.
