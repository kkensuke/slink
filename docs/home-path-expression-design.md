# HOME path expression design

## Status

Design proposal.

This document defines a minimal HOME-directory expression for registry `link` and `target` values while preserving the canonical relative-target model from the relative symlink target work.

The design deliberately treats HOME notation as **registry input syntax**, not as persistent state and not as a general template language.

## Goals

- Allow users to write HOME-relative paths in registry `link` and `target` values.
- Keep `Entry.link` and `Entry.target` concrete after validation.
- Preserve literal `~` symlink targets and ordinary pathname semantics.
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
- guarantee representation of every theoretically valid relative pathname spelling.

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

Instead, slink reserves the canonical relative target forms:

```text
${HOME}
${HOME}/...
```

A canonical relative target in that reserved namespace is unsupported.

This is a representation restriction, not a filesystem claim. A directory named `${HOME}` remains valid; slink simply does not persist a relative symlink target whose canonical text begins with that reserved component.

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

For the reserved case:

```text
readlink() == "${HOME}/foo"
```

the canonical relative target would collide with registry HOME syntax.

The simplest invariant is to reject adoption of that representation:

```text
cannot adopt symlink target "${HOME}/foo":
relative target conflicts with reserved registry HOME syntax
```

The design does not silently reinterpret the observed target as HOME, and it does not invent an escape form.

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

The exact function names are not normative.

The important structural property is that expression expansion stays at the registry boundary and all downstream code consumes concrete values.

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
| registry `target = "./${HOME}/x"` | error after canonicalization enters reserved form |
| `readlink() == "~/x"` during adopt | allowed literal target |
| `readlink() == "${HOME}/x"` during adopt | error: reserved canonical relative target |
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

## Compatibility

Existing registries without `${HOME}` retain their existing interpretation, except for the deliberately reserved relative-target namespace if such an entry already exists.

Literal `~` targets remain valid and unchanged.

No schema migration is required.

Older slink versions will treat `${HOME}` as literal pathname text, so registries using the new syntax require a version that implements this design.

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
8. `adopt` rejects literal leading `${HOME}/...`;
9. relative generation rejects a candidate beginning with `${HOME}/...`;
10. relative generation allows `../${HOME}/...`;
11. pending transaction data contains only the concrete materialized target;
12. recovery remains independent of later HOME changes;
13. slink-generated registry writes never emit `${HOME}`;
14. unrelated registry writes do not rewrite untouched hand-authored HOME expressions.

## Final contract

The feature can be summarized as:

> Registry `link` and `target` values may use `${HOME}` or `${HOME}/...` as a HOME-relative input expression. Expansion occurs once during registry validation. Validated state and transactions contain only concrete canonical paths or targets, and slink never emits `${HOME}` itself. All other `$` and `~` forms are literal. Canonical relative targets beginning with `${HOME}` are reserved and unsupported so that registry interpretation remains unambiguous.

This keeps HOME support at the configuration boundary while preserving the existing relative-target, transaction, and recovery model.
