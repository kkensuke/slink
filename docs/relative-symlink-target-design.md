# Relative symlink target design

## Status

Design proposal. Implementation is intentionally out of scope for this pull request.

## Goal

Add relative symlink support with the smallest change to slink's existing model.

The design should:

- keep the registry at two fields;
- keep CLI path interpretation unchanged;
- reuse existing semantic target comparison and transaction machinery;
- avoid storing meaningless path spelling differences;
- avoid rewriting an already-correct symlink unless the user explicitly asks with `--force`.

## Non-goals

This proposal does not make the registry relocatable.

`link` remains an absolute managed location. Moving or cloning a project can therefore leave the registry pointing at the old link path even when an on-disk relative symlink would remain valid.

Registry relocation or machine-independent link locations are separate problems.

## Core model

A registry entry remains:

```toml
[[link]]
link   = "/Users/me/project/links/config"
target = "../config"
```

The fields mean:

| Field | Meaning |
| --- | --- |
| `link` | Absolute location of the managed symlink |
| `target` | Canonical target text slink intends to materialize |

The target may be absolute or relative.

For a relative target, its path meaning comes from the pair:

```text
(link, target)
```

Conceptually:

```text
reference = reference_target(link, target)
```

No additional persistent field is needed:

```toml
relative = true
style = "relative"
materialized_target = "../config"
```

are all unnecessary.

The registry already contains enough information to recreate the symlink.

## Canonical target spelling

slink should not preserve spelling differences that carry no useful target meaning.

Examples:

```text
./foo          -> foo
foo/./bar      -> foo/bar
foo//bar       -> foo/bar
foo/.          -> foo/
foo/./         -> foo/
```

The canonicalization is deliberately narrow.

It may remove syntactic noise such as:

- interior `.` components;
- redundant ordinary separators;
- a redundant leading `./` on a nontrivial relative path;
- trailing `/.`, normalized to trailing `/`.

It must preserve path features that may affect meaning:

- `..` components;
- absolute versus relative form;
- a trailing `/` directory requirement;
- target references that pass through symlinks;
- any leading-slash behavior that the existing path semantics intentionally preserve.

Standalone values such as `.` must remain representable; canonicalization must never produce an empty target.

Canonicalization is syntactic. It must not resolve the target, follow target symlinks, or collapse `..` merely because a lexical simplification appears possible.

This gives slink one preferred representation without changing reference semantics.

## Design invariants

The implementation should preserve these rules:

1. `link` remains an absolute managed location.
2. `target` is absolute or relative canonical target text.
3. Any code that needs target meaning derives it from `(link, target)`.
4. Normal correctness uses semantic reference comparison.
5. slink-generated registry entries and pending operations use canonical target text.
6. Transaction recovery uses exact target-text comparison when deciding whether filesystem state belongs to a pending operation.
7. An existing semantically correct symlink is not rewritten unless `--force` is supplied.

## Creation

Add a create-only option:

```text
-r, --relative
```

Without it, create keeps the existing behavior and materializes an absolute target.

With it, create materializes a relative target.

Example:

```sh
cd /Users/me/project
slink -r -p config links/config
```

creates:

```text
links/config -> ../config
```

and stores:

```toml
[[link]]
link   = "/Users/me/project/links/config"
target = "../config"
```

### CLI path meaning stays unchanged

Both operands continue to mean paths from the working directory.

`--relative` changes only the target representation that slink materializes.

Conceptually:

```text
CLI target
    |
    | existing cwd-based interpretation
    v
absolute reference
    |
    | if -r, encode from the link's actual containing directory
    v
target text
    |
    | canonicalize harmless spelling noise
    v
registry target / symlink target
```

For:

```sh
cd /Users/me/project
slink -r config links/config
```

`config` first means:

```text
/Users/me/project/config
```

and is then represented from the directory containing `links/config` as:

```text
../config
```

The user never has to calculate `../` manually.

## Relative target generation

Relative target generation must use the existing path semantics instead of blindly applying lexical path-difference rules.

The containing-directory base is conceptually:

```text
base = directory_location(link.parent())
```

The target reference itself must not be canonicalized through the filesystem.

This preserves existing behavior around:

- symlink parents;
- meaningful `..`;
- missing path components;
- targets that traverse symlinks.

### Round-trip verification

A generated relative candidate is accepted only if it preserves the requested reference under the existing interpretation logic:

```rust
reference_target(link, candidate) == original_reference
```

Canonicalization may be applied to the candidate before this check.

If slink cannot produce a safe relative representation, creation fails instead of silently changing meaning.

This reuses `reference_target()` as the authority rather than duplicating path semantics in the relative generator.

## Adoption

`adopt` should observe the current `readlink()` target, canonicalize only meaningless spelling, and store that canonical target.

For example:

```text
links/config -> ../foo/./bar
```

followed by:

```sh
slink adopt links/config
```

stores:

```toml
[[link]]
link   = "/absolute/path/to/links/config"
target = "../foo/bar"
```

### Adopt does not rewrite the filesystem

The existing symlink remains:

```text
links/config -> ../foo/./bar
```

because it already has the same semantic reference.

This keeps `adopt` conceptually small:

```text
observe
-> canonicalize desired representation
-> register
```

It does not become a destructive replacement operation merely to remove spelling noise.

Later, if the link is missing, `fix` recreates the canonical form.

If the user wants the existing symlink rewritten immediately to the canonical registered form, `fix -f` provides that explicit operation.

## Semantic equality

Normal checking continues to compare references rather than raw target strings:

```rust
target_matches(link, actual_target, registered_target)
```

Conceptually:

```text
reference_target(link, actual_target)
    ==
reference_target(link, registered_target)
```

Therefore:

```text
filesystem:
    link -> ../foo/./bar

registry:
    target = "../foo/bar"
```

is healthy when both represent the same reference.

This is reference equality, not final-inode equality. slink must not erase meaningful intermediate symlink references.

## Force and representation enforcement

`-r` and `-f` have separate responsibilities:

- `-r` selects a relative target representation instead of the default absolute representation.
- `-f` allows slink to enforce the requested or registered canonical target text even when an existing symlink is already semantically correct.

Conceptually:

```rust
if actual.target == requested.target {
    Keep
} else if target_matches(link, &actual.target, &requested.target)? {
    if force {
        Replace(actual)
    } else {
        Keep(actual)
    }
} else if force {
    Replace(actual)
} else {
    Conflict
}
```

For example, with:

```text
links/config -> /Users/me/project/config
```

this:

```sh
slink -r config links/config
```

keeps the existing symlink when it already refers to the requested target, but records the relative canonical target for future restoration.

This:

```sh
slink -rf config links/config
```

replaces it immediately with:

```text
links/config -> ../config
```

The reverse is symmetric: ordinary create can register the default absolute representation without rewriting an equivalent relative symlink, while `-f` enforces the requested absolute representation.

No separate conversion command is needed.

## Fix

For a missing symlink, `fix` writes the registered canonical target directly:

```rust
symlink(&entry.target, &link)
```

For an existing semantically matching symlink, ordinary `fix` leaves it untouched even if its spelling differs.

`fix -f` enforces the exact canonical target registered by slink.

Example:

```text
filesystem:
    link -> ../foo/./bar

registry:
    target = "../foo/bar"
```

`fix` leaves it alone.

`fix -f` replaces it with:

```text
link -> ../foo/bar
```

No `--relative` option is needed on `fix`; the registry already contains the desired representation.

## Remove

`remove` continues to use semantic comparison before removing a managed symlink.

A spelling difference such as `../foo/./bar` versus `../foo/bar` does not block removal when the references match.

No representation-specific remove behavior is required.

## Self-reference

Self-reference checks must interpret a relative target from the link location.

Conceptually:

```text
resolved reference = target_path(link, target)
```

and direct self-reference is rejected using that interpreted reference.

The check must not assume that a registry target is absolute.

## Registry validation and persistence

### `link`

The existing rules remain:

- absolute;
- valid link path;
- normalized according to existing link rules;
- included in duplicate/path-key validation.

### `target`

The target:

- may be absolute or relative;
- must be non-empty;
- must not contain NUL;
- is canonicalized only for semantically irrelevant spelling.

slink-generated registry entries are canonical.

A manually edited registry may contain a noncanonical spelling. Validated commands should interpret it through the same target canonicalization before acting on it. A later registry write naturally persists the canonical representation.

Read-only source inspection does not need a new behavior solely for this feature; existing `list` semantics can remain unchanged.

## Output

This proposal does not change the output contract.

The existing distinction remains useful:

- human path output may apply display cleanup for readability;
- `list -o tsv` can expose registry source text exactly as it does today;
- `check` and managed `scan` continue to use their existing registered-target display rules;
- `ACTUAL_TARGET` continues to expose the exact `readlink()` text where the current format already does so.

Because slink-generated target text is canonical, ordinary relative targets such as `../config` display identically with or without cleanup.

An externally observed symlink may still contain spelling such as `../foo/./bar`; that is filesystem observation, not persistent state slink needs to reproduce.

No `output.rs` or path-display contract change is required for relative-link support.

## Transaction recovery

Pending operations contain the canonical target text that slink intends to create.

Normal operation and recovery intentionally use different equality rules:

```text
normal correctness
    semantic reference equality

recovery ownership
    exact pending target-text equality
```

This prevents slink from claiming a semantically equivalent symlink that may have appeared independently during recovery.

### Repeated create recovery

Create should have one shared target-materialization function used by both:

- normal create planning;
- repeated-command matching for a pending create.

Conceptually:

```rust
materialize_create_target(link, target_operand, relative)
```

returns the canonical target text.

Retrying a pending relative create without `-r` therefore produces a different materialized target and is rejected even though `Request` does not need another persisted representation field.

### Representation-only replacement

A forced representation change is an ordinary existing `Replace` transaction.

No new transaction operation is needed.

## Fix dependency ordering

Existing dependency ordering can continue deriving target paths from:

```text
(link, entry.target)
```

Relative registry targets do not require another dependency model.

Code needing target meaning should use the existing link-aware target interpretation instead of assuming `entry.target` is absolute.

## Implementation surface

The intended change remains narrow:

| Area | Change |
| --- | --- |
| `cli.rs` | Add create-only `-r` / `--relative`; keep `-f` as the explicit representation-enforcement switch |
| `paths.rs` | Add canonical target spelling and safe absolute-to-relative target materialization; make self-reference relative-aware |
| `registry.rs` | Allow relative targets and canonicalize target spelling during validation |
| `engine.rs` | Share create target materialization; adopt canonical observed target; allow forced replacement of textually different equivalent symlinks |
| `transaction.rs` | Allow relative canonical pending targets; keep exact recovery matching and existing `Replace` recovery |

No output implementation change is required.

## Compatibility

No schema migration is required.

Existing slink-generated registries already contain normalized absolute targets and retain the same effective behavior.

Hand-edited absolute target spelling such as:

```toml
target = "/a/./b"
```

continues the existing principle that validated path use removes irrelevant spelling rather than treating `/./` as persistent state.

The same principle is extended to relative targets.

Registries containing relative targets require a slink version that implements this design; older versions that require absolute registry targets will reject them.

## Focused tests

New coverage should stay small and reuse existing path and recovery tests.

At minimum:

1. `-r` / `--relative` create writes and registers the expected relative target, and missing-link `fix` restores it.
2. `adopt` of a relative target stores canonical spelling without rewriting an already-correct symlink.
3. Canonical target spelling removes `./`, interior `/./`, redundant ordinary separators, and normalizes trailing `/.` to `/`, while preserving `..` and the trailing-directory requirement.
4. A semantically equivalent absolute/relative or noncanonical/canonical pair matches without `-f`; `-f` rewrites it to the requested or registered canonical representation.
5. Relative generation round-trips correctly when the link parent or target path involves symlinks or missing components.
6. Relative create and forced representation replacement recover through the existing transaction machinery.

Existing tests should continue covering output formatting, dependency ordering, semantic matching, and crash safety.

## Resulting user model

The complete model is:

```text
slink target link
    create/register the default absolute representation

slink -r target link
    create/register a relative representation

slink -rf target link
    enforce that relative representation now

adopt
    remember the symlink's meaning in canonical target spelling
    do not rewrite an already-correct symlink

fix
    recreate missing links from the registered representation

fix -f
    also enforce the registered canonical representation

check / remove
    compare target meaning, not spelling
```

Registry state stays small:

```text
link   = where the symlink lives
target = canonical text slink would write into it
```

That provides relative symlinks without a style field, a second target field, a new conversion command, or a new output contract.
