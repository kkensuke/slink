# HOME path expression design

## Status

Design proposal.

This document defines a minimal HOME-directory expression for registry `link` and `target` values while preserving the canonical relative-target model from the relative symlink target work.

The design deliberately treats HOME notation as **registry input syntax**, not as persistent state and not as a general template language.

## Goals

- Allow users to write HOME-relative paths in registry `link` and `target` values.
- Keep `Entry.link` and `Entry.target` concrete after validation.
- Preserve literal `~` symlink targets and ordinary pathname semantics.
- Preserve the meaning of already-existing symlinks during `adopt` even when their raw target spelling collides with registry expression syntax.
- Keep `adopt`, `fix`, semantic comparison, transactions, and recovery on the existing concrete-target model.
- Avoid a general environment-variable or interpolation language.
- Avoid new persistent fields, enums, transaction variants, or recovery state.
- Keep the implementation small enough to remain a parser/validation feature rather than a new subsystem.

## Non-goals

This design does not:

- make the registry relocatable between different HOME directories;
- introduce general `$VAR` or `${VAR}` expansion;
- introduce shell expansion semantics;
- make CLI operands a template language;
- preserve HOME expressions as generated registry output;
- preserve every observed relative target spelling as the registered representation when that spelling collides with reserved registry syntax.

## Existing model

The relative-target design defines the registry around two values:

```text
link
    managed symlink location

target
    canonical target text slink intends to materialize
```

The effective reference is derived from `(link, target)`.

That model should remain unchanged. In particular, after validation:

```text
Entry.link
    concrete managed symlink location

Entry.target
    concrete canonical target text
```

`Entry.target` must not become an unevaluated expression. This keeps:

- `symlink(&entry.target, &link)`;
- semantic target comparison;
- `fix`;
- `fix -f`;
- pending transactions;
- exact recovery ownership;

on the existing concrete-target representation.

## Registry HOME expression

Only these registry-source forms are special:

```text
${HOME}
${HOME}/...
```

Examples:

```toml
[[link]]
link = "${HOME}/.local/bin/tool"
target = "${HOME}/src/tool"
```

They are expanded to the current HOME directory during registry validation.

For example, if HOME is `/Users/alice`:

```text
"${HOME}"
    -> "/Users/alice"

"${HOME}/src/tool"
    -> "/Users/alice/src/tool"
```

No other form is special.

The following are ordinary pathname text:

```text
~
~/foo
$HOME
$HOME/foo
${USER}
${HOME2}
foo/${HOME}/bar
```

The expression is recognized only when the complete value is exactly `${HOME}` or starts with `${HOME}/`.

## Why `${HOME}` instead of `~`

A symlink target does not assign HOME semantics to `~`.

For example:

```text
link -> ~/source
```

is a relative symlink target whose first pathname component is literally `~`. It is interpreted relative to the symlink's containing directory.

Using `~` as registry HOME syntax would therefore collide directly with a valid and reasonably familiar symlink target representation. It also conflicts with canonical target cleanup:

```text
./~/foo
    -> canonical target
~/foo
```

If `~/foo` were HOME syntax, canonicalization could change the meaning of a hand-edited literal target.

Using `${HOME}` separates the namespaces:

```text
~/foo
    ordinary relative symlink target

${HOME}/foo
    registry HOME expression
```

This preserves literal `~` semantics without an escape rule.

## Why not `$HOME`

Supporting both `$HOME` and `${HOME}` would add syntax without adding capability.

A single braced token gives an explicit boundary and avoids questions such as:

```text
$HOMEfoo
$HOME-suffix
$HOME.src
$USER
```

The registry therefore defines exactly one predefined expression:

```text
${HOME}
```

This is not shell expansion.

## Expansion boundary

HOME expansion occurs exactly once while interpreting registry source.

Conceptually:

```text
registry source
    |
    v
expand_registry_home()
    |
    +-- link   -> link-specific validation/canonicalization
    |
    +-- target -> target-specific canonicalization
    |
    v
validated Entry
```

No HOME expression is carried beyond that point.

The following layers remain unaware of HOME syntax:

```text
planning
transactions
pending state
target_matches
fix
fix -f
recovery
symlink()
```

This prevents an expression/materialized-value split from entering the runtime model.

## Link semantics

For registry `link`:

1. expand `${HOME}` if present;
2. require the resulting path to be absolute;
3. apply the existing link normalization rules.

Example:

```toml
link = "${HOME}/bin/tool"
```

may validate as:

```text
/Users/alice/bin/tool
```

Duplicate detection and other link identity checks operate on the concrete validated path.

## Target semantics

For registry `target`:

1. detect and expand a leading `${HOME}` expression before target canonicalization;
2. otherwise treat the value as ordinary pathname text;
3. apply the existing canonical target rules;
4. reject a canonical relative target that falls into the reserved HOME-expression form described below.

Examples:

```text
"${HOME}/src/tool"
    -> absolute concrete target

"../src/tool"
    -> relative target

"~/src/tool"
    -> literal relative target

"$HOME/src/tool"
    -> literal relative target

"foo/${HOME}/bar"
    -> literal pathname text
```

## Reserved canonical relative targets

There is one unavoidable ambiguity for any string-level expression syntax.

A literal relative symlink target can theoretically be:

```text
${HOME}/foo
```

For example, the filesystem could contain a directory literally named `${HOME}`.

If slink persisted that exact relative target in the registry, the next registry read would interpret it as a HOME expression.

The design intentionally does not add an escape language for this rare case.

Instead, the canonical relative target forms:

```text
${HOME}
${HOME}/...
```

are reserved as **registry persistence spellings**.

This does not mean that an existing filesystem symlink with such a target is unmanageable. It means that slink must not persist that relative spelling verbatim as a concrete registry target.

The response depends on the source:

| Source | Behavior |
| --- | --- |
| registry source whose literal target canonicalizes into the reserved form | reject, because the hand-authored registry target is an explicit representation request |
| `--relative` generation that produces the reserved form | reject, because `--relative` explicitly requests a relative representation |
| `adopt` observing the reserved form from `readlink()` | preserve meaning by storing an equivalent safe absolute target |

This is a registry encoding restriction, not a filesystem restriction. A directory named `${HOME}` remains valid.

## Persistability invariant

Every validated or generated `Entry` that slink may write must be safe to serialize as concrete registry text and read again without changing target meaning.

Conceptually:

```text
validated Entry
    |
    | slink-generated persistence
    v
registry concrete strings
    |
    | parse + validate
    v
same link and target meaning
```

This invariant is stronger and more precise than saying that every reserved-looking filesystem target is unsupported.

A read-only validation does **not** rewrite registry source merely because canonicalization changes the in-memory spelling.

For example:

```toml
target = "./${HOME}/foo"
```

is initially ordinary literal pathname text because it does not begin with the registry expression token.

Canonicalization gives:

```text
${HOME}/foo
```

The source file is still unchanged at this point.

The problem is that if slink accepted that value as a concrete validated target, a later operation that creates or updates that entry would persist:

```toml
target = "${HOME}/foo"
```

and a fresh registry load would interpret it as a HOME expression.

The validated `Entry` stores only canonical target text; it does not retain provenance saying that the value originally came from `./${HOME}/foo`. Adding such provenance would reintroduce the expression/materialized-state split this design is intended to avoid.

Therefore literal registry input that canonicalizes into the reserved form is rejected during validation. The rejection establishes closure under future slink-generated persistence; it is not because validation itself immediately rewrites the file.

## Canonicalization order

Expression recognition must happen before target canonicalization.

For example:

```text
"${HOME}/foo"
    -> HOME expression
    -> expand
    -> canonicalize concrete absolute target
```

But:

```text
"./${HOME}/foo"
```

is not a HOME expression at the input boundary.

After canonicalization it would become:

```text
${HOME}/foo
```

which is reserved, so validation fails.

This keeps canonicalization deterministic and avoids context-sensitive exceptions such as "preserve leading `./` only before a HOME-looking token."

## Literal `~` behavior

The following remain valid literal target representations:

```text
~/foo
./~/foo
../~/foo
```

Canonicalization may still remove syntactic noise according to the normal target rules, but no resulting leading `~` form has HOME semantics.

This preserves existing filesystem behavior and allows `adopt` to retain ordinary literal `~` targets.

## CLI behavior

This design does not add `${HOME}` parsing to CLI operands.

CLI path interpretation keeps its existing rules.

In particular:

- shells may expand `~` before slink is invoked;
- any existing slink CLI HOME shorthand can remain as-is;
- `${HOME}` passed as a CLI operand is not introduced as new syntax by this design.

The special token belongs to **registry source syntax only**.

This boundary keeps the feature narrow and avoids making CLI parsing, shell-like expansion, and registry parsing share a new expression language.

## slink-generated registry output

slink never emits `${HOME}` when it writes a new or updated value.

A hand-edited entry may contain:

```toml
target = "${HOME}/src/tool"
```

and validate to:

```text
/Users/alice/src/tool
```

If slink later creates or updates that target field, it persists the concrete canonical representation:

```toml
target = "/Users/alice/src/tool"
```

Therefore:

```text
${HOME}
    accepted input syntax

${HOME}
    not a canonical persistence format generated by slink
```

Validation should remain an in-memory interpretation step. Unrelated registry mutations should not rewrite untouched source entries merely to replace hand-authored `${HOME}`.

## HOME changes

A hand-authored `${HOME}` expression is evaluated during each fresh registry validation using the current HOME value.

Once an operation crosses into concrete validated state, HOME is no longer consulted.

Conceptually:

```text
registry source expression
    |
    | fresh validation
    v
concrete Entry
    |
    | transaction boundary
    v
concrete Pending state
```

A recovery operation therefore does not reinterpret HOME and does not depend on whether HOME changed after the transaction was recorded.

## `adopt`

`readlink()` output is always filesystem data, never registry expression syntax.

Examples:

```text
readlink() == "~/foo"
    -> ordinary target
    -> may be adopted normally
```

For a target whose **canonical** spelling falls into the reserved persistence namespace:

```text
readlink() == "${HOME}/foo"

or

readlink() == "./${HOME}/foo"
    -> canonical target "${HOME}/foo"
```

`adopt` should not reject the symlink merely because its existing relative spelling cannot be stored verbatim.

Instead, it should use the existing link-aware reference interpretation to derive the same target meaning as a safe absolute target and store that concrete absolute representation.

Conceptually:

```text
observed readlink target
    |
    | canonical_target()
    v
canonical observed target
    |
    | reserved?
    +---- no  -> store canonical observed target
    |
    +---- yes -> canonical_reference(link, observed target)
                 -> store safe absolute target
```

For example, if the link's effective containing directory is `/work`:

```text
readlink() == "${HOME}/foo"
    -> filesystem meaning: /work/${HOME}/foo
    -> registry target:   /work/${HOME}/foo
```

The `${HOME}` component in the resulting absolute path is embedded after the leading `/`, so it is ordinary pathname text and cannot be confused with the registry expression.

The conversion must reuse the existing link-aware reference helper rather than naively doing only `link.parent().join(raw)`. The relative-target design already makes the link's effective containing directory authoritative in order to preserve behavior around symlink parents and meaningful path traversal.

`adopt` still does not rewrite the filesystem.

This exceptional representation conversion has two consequences:

- ordinary `fix` leaves the existing relative symlink untouched while it remains semantically correct;
- if the link is later missing, or if the user runs `fix -f`, slink materializes the registered safe absolute representation rather than recreating the reserved relative spelling.

That is consistent with `adopt` remembering target meaning rather than promising byte-for-byte preservation of every observed target representation.

## `--relative`

Relative target generation follows the existing relative-target algorithm and round-trip verification.

After producing the canonical relative candidate, slink checks the reserved namespace.

Example:

```text
candidate = "../${HOME}/foo"
    -> allowed

candidate = "${HOME}/foo"
    -> reserved
    -> error
```

Suggested error:

```text
cannot represent target relatively:
relative target "${HOME}/foo" conflicts with registry HOME syntax;
omit --relative to use an absolute target
```

The command should fail rather than silently fall back to an absolute target. `--relative` explicitly requests a representation, and silent fallback would weaken representation enforcement semantics.

## `fix`, `-f`, and semantic comparison

No special HOME logic is required after validation.

A registry HOME expression has already become a concrete canonical target before planning.

Therefore:

- normal target health remains based on semantic target comparison;
- exact representation enforcement with `-f` remains unchanged;
- missing-link `fix` writes the concrete registered target;
- an adopted reserved relative spelling may therefore be recreated or force-normalized as its safe absolute registered representation;
- transaction ownership/recovery remains exact target-text comparison.

The HOME feature does not introduce a second target representation into these paths.

## Transaction and recovery model

No persistent HOME-specific state is added.

In particular, do not add:

```text
home: bool
expression: String
materialized_target: String
target_kind
```

to registry entries, plans, requests, or pending transactions.

Pending state contains the same concrete target representation used by the existing relative-target design.

This preserves the important distinction:

```text
steady-state correctness
    -> semantic target equality

transaction ownership/recovery
    -> exact concrete target-text equality
```

## Implementation shape

The feature should require only small path/registry helpers.

Conceptually:

```rust
fn expand_registry_home(s: &str) -> Result<Option<PathBuf>> {
    if s == "${HOME}" {
        Ok(Some(home()?))
    } else if let Some(rest) = s.strip_prefix("${HOME}/") {
        Ok(Some(home()?.join(rest)))
    } else {
        Ok(None)
    }
}
```

and:

```rust
fn is_reserved_home_target(target: &str) -> bool {
    target == "${HOME}" || target.starts_with("${HOME}/")
}
```

Registry target interpretation is conceptually:

```rust
fn registry_target(s: &str) -> Result<String> {
    if let Some(expanded) = expand_registry_home(s)? {
        return canonical_target(expanded);
    }

    let target = canonical_target(s)?;

    if is_reserved_home_target(&target) {
        bail!("relative target conflicts with reserved registry HOME syntax");
    }

    Ok(target)
}
```

Adoption uses the same reserved-form predicate but a different response because its input is observed filesystem state rather than an explicit representation request:

```rust
fn adopt_target(link: &Path, raw: &str) -> Result<String> {
    let target = canonical_target(raw)?;

    if !is_reserved_home_target(&target) {
        return Ok(target);
    }

    // Reuse the existing link-aware reference semantics from the
    // relative-target model. The result is a safe absolute target.
    canonical_reference(link, &target)
}
```

The exact function names and visibility are not normative.

The important structural property is that expression expansion stays at the registry boundary and all downstream code consumes concrete values. The reserved-form check is shared; only the boundary-specific response differs.

## Validation matrix

| Input/source | Meaning |
|---|---|
| registry `link = "${HOME}/bin/x"` | HOME expansion, then absolute link validation |
| registry `target = "${HOME}/src/x"` | HOME expansion to absolute target |
| registry `target = "../src/x"` | ordinary relative target |
| registry `target = "~/src/x"` | literal `~` relative target |
| registry `target = "$HOME/src/x"` | literal `$HOME` pathname |
| registry `target = "${USER}/src/x"` | literal pathname |
| registry `target = "foo/${HOME}/x"` | literal embedded component |
| registry `target = "./${HOME}/x"` | error after canonicalization enters reserved persistence form |
| `readlink() == "~/x"` during adopt | allowed literal target |
| `readlink() == "${HOME}/x"` during adopt | store equivalent safe absolute target; do not rewrite symlink |
| `readlink() == "./${HOME}/x"` during adopt | canonical form is reserved, so store equivalent safe absolute target |
| generated relative target `../${HOME}/x` | allowed |
| generated relative target `${HOME}/x` | error: reserved canonical relative target |

## Error principles

Errors should describe the representation conflict, not claim that the pathname is invalid.

Good:

```text
relative target "${HOME}/foo" conflicts with reserved registry HOME syntax
```

Avoid:

```text
invalid path
```

because the underlying pathname may be perfectly valid outside the registry representation.

This error applies when the user explicitly requests an unpersistable representation, such as hand-authored literal registry input or `--relative` output. `adopt` instead re-encodes an observed reserved target as a safe absolute representation.

## Compatibility

Existing registries without a leading `${HOME}` target or link expression retain their existing interpretation.

Literal `~` targets remain valid and unchanged.

No schema migration is required.

There is one intentional syntax-compatibility boundary: an older slink version treats a source value beginning exactly with `${HOME}` as literal pathname text, while a version implementing this design treats it as the HOME expression. A pre-existing registry that intentionally used a literal leading `${HOME}` value must therefore be rewritten to an unambiguous representation, typically an equivalent absolute path, before relying on the new interpretation.

Older slink versions likewise do not understand the new expression semantics, so registries using `${HOME}` as an expression require a version that implements this design.

Because slink-generated output does not emit HOME expressions, the feature primarily affects explicitly hand-authored registry values.

## Alternatives considered

### Use `~` for HOME

Rejected because it directly conflicts with real relative symlink targets such as:

```text
~/foo
```

and creates canonicalization/escape problems for forms such as `./~/foo`.

### Use `$HOME`

Rejected because it adds no capability beyond `${HOME}` while introducing less explicit token boundaries and stronger shell-variable expectations.

### Support both `$HOME` and `${HOME}`

Rejected as unnecessary syntax surface.

### Use `${SLINK_HOME}`

This would make the token more obviously slink-specific and would make accidental literal collisions even less likely.

It is not chosen because it is longer, less immediately readable as "the user's home directory", and looks like a reference to an environment variable named `SLINK_HOME` even though the proposed feature reads the normal HOME directory.

It also does not remove the structural collision: a literal relative target named `${SLINK_HOME}/foo` would still need the same persistence rule. It only makes that collision rarer.

`${HOME}` therefore keeps the user-facing syntax familiar while the reserved persistence invariant handles the rare ambiguity explicitly.

### Add a `./` or backslash escape

Rejected because canonical target normalization would need context-sensitive exceptions, complicating a representation model that is otherwise deterministic.

### Store HOME expressions persistently

Rejected because it would split registry expression from materialized symlink target and force planning, fixing, transaction, output, and recovery code to distinguish the two.

### Use typed TOML

For example:

```toml
target = { home = "src/foo" }
```

This completely separates literal strings from expressions and can represent every ordinary string without reserving a namespace.

It is not chosen because the additional schema/serde surface is disproportionate to the narrow HOME convenience being added.

If exact representation of every theoretically valid relative target becomes a hard requirement in the future, typed syntax is the clean alternative.

## Focused tests

At minimum, implementation should cover:

1. registry `link = "${HOME}/..."` expands and validates;
2. registry `target = "${HOME}/..."` expands to a concrete absolute target;
3. `target = "~/..."` remains literal;
4. `target = "$HOME/..."` remains literal;
5. embedded `${HOME}` remains literal;
6. canonical `./${HOME}/...` is rejected as reserved;
7. `adopt` preserves literal `~/...`;
8. `adopt` converts an observed canonical reserved target such as `${HOME}/...` or `./${HOME}/...` to a semantically equivalent safe absolute registered target without rewriting the existing symlink;
9. that adopt conversion uses the existing link-aware reference semantics and remains correct when the link parent is reached through symlinks;
10. ordinary `fix` leaves the adopted reserved-spelling symlink untouched while it is semantically correct, while missing-link `fix` and `fix -f` materialize the registered safe absolute representation;
11. relative generation rejects a candidate beginning with `${HOME}/...`;
12. relative generation allows `../${HOME}/...`;
13. pending transaction data contains only the concrete materialized target;
14. recovery remains independent of later HOME changes;
15. slink-generated registry writes never emit `${HOME}`;
16. unrelated registry writes do not rewrite untouched hand-authored HOME expressions.

## Final contract

The feature can be summarized as:

> Registry `link` and `target` values may use `${HOME}` or `${HOME}/...` as a HOME-relative input expression. Expansion occurs once during registry validation. Validated state and transactions contain only concrete canonical paths or targets, and slink never emits `${HOME}` itself. All other `# HOME path expression design

## Status

Design proposal.

This document defines a minimal HOME-directory expression for registry `link` and `target` values while preserving the canonical relative-target model from the relative symlink target work.

The design deliberately treats HOME notation as **registry input syntax**, not as persistent state and not as a general template language.

## Goals

- Allow users to write HOME-relative paths in registry `link` and `target` values.
- Keep `Entry.link` and `Entry.target` concrete after validation.
- Preserve literal `~` symlink targets and ordinary pathname semantics.
- Preserve the meaning of already-existing symlinks during `adopt` even when their raw target spelling collides with registry expression syntax.
- Keep `adopt`, `fix`, semantic comparison, transactions, and recovery on the existing concrete-target model.
- Avoid a general environment-variable or interpolation language.
- Avoid new persistent fields, enums, transaction variants, or recovery state.
- Keep the implementation small enough to remain a parser/validation feature rather than a new subsystem.

## Non-goals

This design does not:

- make the registry relocatable between different HOME directories;
- introduce general `$VAR` or `${VAR}` expansion;
- introduce shell expansion semantics;
- make CLI operands a template language;
- preserve HOME expressions as generated registry output;
- preserve every observed relative target spelling as the registered representation when that spelling collides with reserved registry syntax.

## Existing model

The relative-target design defines the registry around two values:

```text
link
    managed symlink location

target
    canonical target text slink intends to materialize
```

The effective reference is derived from `(link, target)`.

That model should remain unchanged. In particular, after validation:

```text
Entry.link
    concrete managed symlink location

Entry.target
    concrete canonical target text
```

`Entry.target` must not become an unevaluated expression. This keeps:

- `symlink(&entry.target, &link)`;
- semantic target comparison;
- `fix`;
- `fix -f`;
- pending transactions;
- exact recovery ownership;

on the existing concrete-target representation.

## Registry HOME expression

Only these registry-source forms are special:

```text
${HOME}
${HOME}/...
```

Examples:

```toml
[[link]]
link = "${HOME}/.local/bin/tool"
target = "${HOME}/src/tool"
```

They are expanded to the current HOME directory during registry validation.

For example, if HOME is `/Users/alice`:

```text
"${HOME}"
    -> "/Users/alice"

"${HOME}/src/tool"
    -> "/Users/alice/src/tool"
```

No other form is special.

The following are ordinary pathname text:

```text
~
~/foo
$HOME
$HOME/foo
${USER}
${HOME2}
foo/${HOME}/bar
```

The expression is recognized only when the complete value is exactly `${HOME}` or starts with `${HOME}/`.

## Why `${HOME}` instead of `~`

A symlink target does not assign HOME semantics to `~`.

For example:

```text
link -> ~/source
```

is a relative symlink target whose first pathname component is literally `~`. It is interpreted relative to the symlink's containing directory.

Using `~` as registry HOME syntax would therefore collide directly with a valid and reasonably familiar symlink target representation. It also conflicts with canonical target cleanup:

```text
./~/foo
    -> canonical target
~/foo
```

If `~/foo` were HOME syntax, canonicalization could change the meaning of a hand-edited literal target.

Using `${HOME}` separates the namespaces:

```text
~/foo
    ordinary relative symlink target

${HOME}/foo
    registry HOME expression
```

This preserves literal `~` semantics without an escape rule.

## Why not `$HOME`

Supporting both `$HOME` and `${HOME}` would add syntax without adding capability.

A single braced token gives an explicit boundary and avoids questions such as:

```text
$HOMEfoo
$HOME-suffix
$HOME.src
$USER
```

The registry therefore defines exactly one predefined expression:

```text
${HOME}
```

This is not shell expansion.

## Expansion boundary

HOME expansion occurs exactly once while interpreting registry source.

Conceptually:

```text
registry source
    |
    v
expand_registry_home()
    |
    +-- link   -> link-specific validation/canonicalization
    |
    +-- target -> target-specific canonicalization
    |
    v
validated Entry
```

No HOME expression is carried beyond that point.

The following layers remain unaware of HOME syntax:

```text
planning
transactions
pending state
target_matches
fix
fix -f
recovery
symlink()
```

This prevents an expression/materialized-value split from entering the runtime model.

## Link semantics

For registry `link`:

1. expand `${HOME}` if present;
2. require the resulting path to be absolute;
3. apply the existing link normalization rules.

Example:

```toml
link = "${HOME}/bin/tool"
```

may validate as:

```text
/Users/alice/bin/tool
```

Duplicate detection and other link identity checks operate on the concrete validated path.

## Target semantics

For registry `target`:

1. detect and expand a leading `${HOME}` expression before target canonicalization;
2. otherwise treat the value as ordinary pathname text;
3. apply the existing canonical target rules;
4. reject a canonical relative target that falls into the reserved HOME-expression form described below.

Examples:

```text
"${HOME}/src/tool"
    -> absolute concrete target

"../src/tool"
    -> relative target

"~/src/tool"
    -> literal relative target

"$HOME/src/tool"
    -> literal relative target

"foo/${HOME}/bar"
    -> literal pathname text
```

## Reserved canonical relative targets

There is one unavoidable ambiguity for any string-level expression syntax.

A literal relative symlink target can theoretically be:

```text
${HOME}/foo
```

For example, the filesystem could contain a directory literally named `${HOME}`.

If slink persisted that exact relative target in the registry, the next registry read would interpret it as a HOME expression.

The design intentionally does not add an escape language for this rare case.

Instead, the canonical relative target forms:

```text
${HOME}
${HOME}/...
```

are reserved as **registry persistence spellings**.

This does not mean that an existing filesystem symlink with such a target is unmanageable. It means that slink must not persist that relative spelling verbatim as a concrete registry target.

The response depends on the source:

| Source | Behavior |
| --- | --- |
| registry source whose literal target canonicalizes into the reserved form | reject, because the hand-authored registry target is an explicit representation request |
| `--relative` generation that produces the reserved form | reject, because `--relative` explicitly requests a relative representation |
| `adopt` observing the reserved form from `readlink()` | preserve meaning by storing an equivalent safe absolute target |

This is a registry encoding restriction, not a filesystem restriction. A directory named `${HOME}` remains valid.

## Persistability invariant

Every validated or generated `Entry` that slink may write must be safe to serialize as concrete registry text and read again without changing target meaning.

Conceptually:

```text
validated Entry
    |
    | slink-generated persistence
    v
registry concrete strings
    |
    | parse + validate
    v
same link and target meaning
```

This invariant is stronger and more precise than saying that every reserved-looking filesystem target is unsupported.

A read-only validation does **not** rewrite registry source merely because canonicalization changes the in-memory spelling.

For example:

```toml
target = "./${HOME}/foo"
```

is initially ordinary literal pathname text because it does not begin with the registry expression token.

Canonicalization gives:

```text
${HOME}/foo
```

The source file is still unchanged at this point.

The problem is that if slink accepted that value as a concrete validated target, a later operation that creates or updates that entry would persist:

```toml
target = "${HOME}/foo"
```

and a fresh registry load would interpret it as a HOME expression.

The validated `Entry` stores only canonical target text; it does not retain provenance saying that the value originally came from `./${HOME}/foo`. Adding such provenance would reintroduce the expression/materialized-state split this design is intended to avoid.

Therefore literal registry input that canonicalizes into the reserved form is rejected during validation. The rejection establishes closure under future slink-generated persistence; it is not because validation itself immediately rewrites the file.

## Canonicalization order

Expression recognition must happen before target canonicalization.

For example:

```text
"${HOME}/foo"
    -> HOME expression
    -> expand
    -> canonicalize concrete absolute target
```

But:

```text
"./${HOME}/foo"
```

is not a HOME expression at the input boundary.

After canonicalization it would become:

```text
${HOME}/foo
```

which is reserved, so validation fails.

This keeps canonicalization deterministic and avoids context-sensitive exceptions such as "preserve leading `./` only before a HOME-looking token."

## Literal `~` behavior

The following remain valid literal target representations:

```text
~/foo
./~/foo
../~/foo
```

Canonicalization may still remove syntactic noise according to the normal target rules, but no resulting leading `~` form has HOME semantics.

This preserves existing filesystem behavior and allows `adopt` to retain ordinary literal `~` targets.

## CLI behavior

This design does not add `${HOME}` parsing to CLI operands.

CLI path interpretation keeps its existing rules.

In particular:

- shells may expand `~` before slink is invoked;
- any existing slink CLI HOME shorthand can remain as-is;
- `${HOME}` passed as a CLI operand is not introduced as new syntax by this design.

The special token belongs to **registry source syntax only**.

This boundary keeps the feature narrow and avoids making CLI parsing, shell-like expansion, and registry parsing share a new expression language.

## slink-generated registry output

slink never emits `${HOME}` when it writes a new or updated value.

A hand-edited entry may contain:

```toml
target = "${HOME}/src/tool"
```

and validate to:

```text
/Users/alice/src/tool
```

If slink later creates or updates that target field, it persists the concrete canonical representation:

```toml
target = "/Users/alice/src/tool"
```

Therefore:

```text
${HOME}
    accepted input syntax

${HOME}
    not a canonical persistence format generated by slink
```

Validation should remain an in-memory interpretation step. Unrelated registry mutations should not rewrite untouched source entries merely to replace hand-authored `${HOME}`.

## HOME changes

A hand-authored `${HOME}` expression is evaluated during each fresh registry validation using the current HOME value.

Once an operation crosses into concrete validated state, HOME is no longer consulted.

Conceptually:

```text
registry source expression
    |
    | fresh validation
    v
concrete Entry
    |
    | transaction boundary
    v
concrete Pending state
```

A recovery operation therefore does not reinterpret HOME and does not depend on whether HOME changed after the transaction was recorded.

## `adopt`

`readlink()` output is always filesystem data, never registry expression syntax.

Examples:

```text
readlink() == "~/foo"
    -> ordinary target
    -> may be adopted normally
```

For a target whose **canonical** spelling falls into the reserved persistence namespace:

```text
readlink() == "${HOME}/foo"

or

readlink() == "./${HOME}/foo"
    -> canonical target "${HOME}/foo"
```

`adopt` should not reject the symlink merely because its existing relative spelling cannot be stored verbatim.

Instead, it should use the existing link-aware reference interpretation to derive the same target meaning as a safe absolute target and store that concrete absolute representation.

Conceptually:

```text
observed readlink target
    |
    | canonical_target()
    v
canonical observed target
    |
    | reserved?
    +---- no  -> store canonical observed target
    |
    +---- yes -> canonical_reference(link, observed target)
                 -> store safe absolute target
```

For example, if the link's effective containing directory is `/work`:

```text
readlink() == "${HOME}/foo"
    -> filesystem meaning: /work/${HOME}/foo
    -> registry target:   /work/${HOME}/foo
```

The `${HOME}` component in the resulting absolute path is embedded after the leading `/`, so it is ordinary pathname text and cannot be confused with the registry expression.

The conversion must reuse the existing link-aware reference helper rather than naively doing only `link.parent().join(raw)`. The relative-target design already makes the link's effective containing directory authoritative in order to preserve behavior around symlink parents and meaningful path traversal.

`adopt` still does not rewrite the filesystem.

This exceptional representation conversion has two consequences:

- ordinary `fix` leaves the existing relative symlink untouched while it remains semantically correct;
- if the link is later missing, or if the user runs `fix -f`, slink materializes the registered safe absolute representation rather than recreating the reserved relative spelling.

That is consistent with `adopt` remembering target meaning rather than promising byte-for-byte preservation of every observed target representation.

## `--relative`

Relative target generation follows the existing relative-target algorithm and round-trip verification.

After producing the canonical relative candidate, slink checks the reserved namespace.

Example:

```text
candidate = "../${HOME}/foo"
    -> allowed

candidate = "${HOME}/foo"
    -> reserved
    -> error
```

Suggested error:

```text
cannot represent target relatively:
relative target "${HOME}/foo" conflicts with registry HOME syntax;
omit --relative to use an absolute target
```

The command should fail rather than silently fall back to an absolute target. `--relative` explicitly requests a representation, and silent fallback would weaken representation enforcement semantics.

## `fix`, `-f`, and semantic comparison

No special HOME logic is required after validation.

A registry HOME expression has already become a concrete canonical target before planning.

Therefore:

- normal target health remains based on semantic target comparison;
- exact representation enforcement with `-f` remains unchanged;
- missing-link `fix` writes the concrete registered target;
- an adopted reserved relative spelling may therefore be recreated or force-normalized as its safe absolute registered representation;
- transaction ownership/recovery remains exact target-text comparison.

The HOME feature does not introduce a second target representation into these paths.

## Transaction and recovery model

No persistent HOME-specific state is added.

In particular, do not add:

```text
home: bool
expression: String
materialized_target: String
target_kind
```

to registry entries, plans, requests, or pending transactions.

Pending state contains the same concrete target representation used by the existing relative-target design.

This preserves the important distinction:

```text
steady-state correctness
    -> semantic target equality

transaction ownership/recovery
    -> exact concrete target-text equality
```

## Implementation shape

The feature should require only small path/registry helpers.

Conceptually:

```rust
fn expand_registry_home(s: &str) -> Result<Option<PathBuf>> {
    if s == "${HOME}" {
        Ok(Some(home()?))
    } else if let Some(rest) = s.strip_prefix("${HOME}/") {
        Ok(Some(home()?.join(rest)))
    } else {
        Ok(None)
    }
}
```

and:

```rust
fn is_reserved_home_target(target: &str) -> bool {
    target == "${HOME}" || target.starts_with("${HOME}/")
}
```

Registry target interpretation is conceptually:

```rust
fn registry_target(s: &str) -> Result<String> {
    if let Some(expanded) = expand_registry_home(s)? {
        return canonical_target(expanded);
    }

    let target = canonical_target(s)?;

    if is_reserved_home_target(&target) {
        bail!("relative target conflicts with reserved registry HOME syntax");
    }

    Ok(target)
}
```

Adoption uses the same reserved-form predicate but a different response because its input is observed filesystem state rather than an explicit representation request:

```rust
fn adopt_target(link: &Path, raw: &str) -> Result<String> {
    let target = canonical_target(raw)?;

    if !is_reserved_home_target(&target) {
        return Ok(target);
    }

    // Reuse the existing link-aware reference semantics from the
    // relative-target model. The result is a safe absolute target.
    canonical_reference(link, &target)
}
```

The exact function names and visibility are not normative.

The important structural property is that expression expansion stays at the registry boundary and all downstream code consumes concrete values. The reserved-form check is shared; only the boundary-specific response differs.

## Validation matrix

| Input/source | Meaning |
|---|---|
| registry `link = "${HOME}/bin/x"` | HOME expansion, then absolute link validation |
| registry `target = "${HOME}/src/x"` | HOME expansion to absolute target |
| registry `target = "../src/x"` | ordinary relative target |
| registry `target = "~/src/x"` | literal `~` relative target |
| registry `target = "$HOME/src/x"` | literal `$HOME` pathname |
| registry `target = "${USER}/src/x"` | literal pathname |
| registry `target = "foo/${HOME}/x"` | literal embedded component |
| registry `target = "./${HOME}/x"` | error after canonicalization enters reserved persistence form |
| `readlink() == "~/x"` during adopt | allowed literal target |
| `readlink() == "${HOME}/x"` during adopt | store equivalent safe absolute target; do not rewrite symlink |
| `readlink() == "./${HOME}/x"` during adopt | canonical form is reserved, so store equivalent safe absolute target |
| generated relative target `../${HOME}/x` | allowed |
| generated relative target `${HOME}/x` | error: reserved canonical relative target |

## Error principles

Errors should describe the representation conflict, not claim that the pathname is invalid.

Good:

```text
relative target "${HOME}/foo" conflicts with reserved registry HOME syntax
```

Avoid:

```text
invalid path
```

because the underlying pathname may be perfectly valid outside the registry representation.

This error applies when the user explicitly requests an unpersistable representation, such as hand-authored literal registry input or `--relative` output. `adopt` instead re-encodes an observed reserved target as a safe absolute representation.

## Compatibility

Existing registries without a leading `${HOME}` target or link expression retain their existing interpretation.

Literal `~` targets remain valid and unchanged.

No schema migration is required.

There is one intentional syntax-compatibility boundary: an older slink version treats a source value beginning exactly with `${HOME}` as literal pathname text, while a version implementing this design treats it as the HOME expression. A pre-existing registry that intentionally used a literal leading `${HOME}` value must therefore be rewritten to an unambiguous representation, typically an equivalent absolute path, before relying on the new interpretation.

Older slink versions likewise do not understand the new expression semantics, so registries using `${HOME}` as an expression require a version that implements this design.

Because slink-generated output does not emit HOME expressions, the feature primarily affects explicitly hand-authored registry values.

## Alternatives considered

### Use `~` for HOME

Rejected because it directly conflicts with real relative symlink targets such as:

```text
~/foo
```

and creates canonicalization/escape problems for forms such as `./~/foo`.

### Use `$HOME`

Rejected because it adds no capability beyond `${HOME}` while introducing less explicit token boundaries and stronger shell-variable expectations.

### Support both `$HOME` and `${HOME}`

Rejected as unnecessary syntax surface.

### Use `${SLINK_HOME}`

This would make the token more obviously slink-specific and would make accidental literal collisions even less likely.

It is not chosen because it is longer, less immediately readable as "the user's home directory", and looks like a reference to an environment variable named `SLINK_HOME` even though the proposed feature reads the normal HOME directory.

It also does not remove the structural collision: a literal relative target named `${SLINK_HOME}/foo` would still need the same persistence rule. It only makes that collision rarer.

`${HOME}` therefore keeps the user-facing syntax familiar while the reserved persistence invariant handles the rare ambiguity explicitly.

### Add a `./` or backslash escape

Rejected because canonical target normalization would need context-sensitive exceptions, complicating a representation model that is otherwise deterministic.

### Store HOME expressions persistently

Rejected because it would split registry expression from materialized symlink target and force planning, fixing, transaction, output, and recovery code to distinguish the two.

### Use typed TOML

For example:

```toml
target = { home = "src/foo" }
```

This completely separates literal strings from expressions and can represent every ordinary string without reserving a namespace.

It is not chosen because the additional schema/serde surface is disproportionate to the narrow HOME convenience being added.

If exact representation of every theoretically valid relative target becomes a hard requirement in the future, typed syntax is the clean alternative.

## Focused tests

At minimum, implementation should cover:

1. registry `link = "${HOME}/..."` expands and validates;
2. registry `target = "${HOME}/..."` expands to a concrete absolute target;
3. `target = "~/..."` remains literal;
4. `target = "$HOME/..."` remains literal;
5. embedded `${HOME}` remains literal;
6. canonical `./${HOME}/...` is rejected as reserved;
7. `adopt` preserves literal `~/...`;
8. `adopt` converts an observed canonical reserved target such as `${HOME}/...` or `./${HOME}/...` to a semantically equivalent safe absolute registered target without rewriting the existing symlink;
9. that adopt conversion uses the existing link-aware reference semantics and remains correct when the link parent is reached through symlinks;
10. ordinary `fix` leaves the adopted reserved-spelling symlink untouched while it is semantically correct, while missing-link `fix` and `fix -f` materialize the registered safe absolute representation;
11. relative generation rejects a candidate beginning with `${HOME}/...`;
12. relative generation allows `../${HOME}/...`;
13. pending transaction data contains only the concrete materialized target;
14. recovery remains independent of later HOME changes;
15. slink-generated registry writes never emit `${HOME}`;
16. unrelated registry writes do not rewrite untouched hand-authored HOME expressions.

## Final contract

The feature can be summarized as:

 and `~` forms are literal. Canonical relative target spellings beginning with `${HOME}` are reserved for registry persistence: explicit registry literals and `--relative` generation reject that representation, while `adopt` preserves an already-existing symlink's meaning by registering an equivalent safe absolute target without rewriting the filesystem.

This keeps HOME support at the configuration boundary while preserving the existing relative-target, transaction, and recovery model.
