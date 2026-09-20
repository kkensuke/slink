# HOME path expression design

## Status

Design proposal.

This document defines a minimal HOME-directory expression for registry `link` and `target` values while preserving the canonical relative-target model from #28.

The central rule is:

```text
registry source may contain a HOME expression
        ↓ validation
runtime Entry is always concrete
        ↓ registry serialization
the configured registry write style chooses concrete or HOME-expression spelling
```

HOME notation is never carried as runtime target state.

## Goals

- Allow HOME-relative expressions in registry `link` and `target` values.
- Keep `Entry.link` and `Entry.target` concrete after validation.
- Let a registry choose whether slink-generated HOME paths are written as concrete absolute paths or as `${HOME}` expressions.
- Keep that choice at registry scope so shared registries do not change spelling according to each user's personal preference.
- Preserve literal `~` symlink targets and ordinary pathname semantics.
- Preserve the meaning of already-existing symlinks during `adopt` even when their raw target spelling collides with registry expression syntax.
- Keep `fix`, semantic comparison, transactions, and recovery on the existing concrete-target model.
- Avoid a general environment-variable or interpolation language.
- Avoid per-entry expression state, new transaction variants, or recovery state.
- Avoid registry-wide normalization as a side effect of ordinary commands.

## Non-goals

This design does not:

- introduce general `$VAR` or `${VAR}` expansion;
- introduce shell expansion semantics;
- make CLI operands a template language;
- promise general machine-independent registry relocation;
- preserve every observed relative target spelling when that spelling collides with reserved registry syntax;
- automatically rewrite all existing entries when the registry write preference changes;
- add a registry formatter command.

`${HOME}` can make individual HOME-based entries easier to share, but it is not a general relocation mechanism.

## Existing runtime model

The relative-target design keeps each validated entry conceptually as:

```text
link
    concrete absolute managed symlink location

target
    concrete canonical target text slink intends to materialize
```

The target may be absolute or relative, and relative target meaning is derived from `(link, target)`.

This design does not change that model.

After validation:

```text
Entry.link
    concrete absolute path

Entry.target
    concrete canonical target text
```

In particular, runtime state does not carry:

```text
home_expression
source_expression
target_style
materialized_target
```

as per-entry semantic state.

That keeps:

- `target_matches`;
- `fix`;
- `fix -f`;
- pending transactions;
- exact recovery ownership;
- symlink materialization;

on the existing concrete representation.

## Registry HOME expression

Only these registry-source forms are special:

```text
${HOME}
${HOME}/...
```

Example:

```toml
[[link]]
link = "${HOME}/.local/bin/tool"
target = "${HOME}/src/tool"
```

If HOME is `/Users/alice`, validation interprets them as:

```text
"${HOME}"
    -> "/Users/alice"

"${HOME}/src/tool"
    -> "/Users/alice/src/tool"
```

No other form is special.

These remain ordinary pathname text:

```text
~
~/foo
$HOME
$HOME/foo
${USER}
${HOME2}
foo/${HOME}/bar
```

The token is recognized only when the complete value is exactly `${HOME}` or starts with `${HOME}/`.

This is a slink registry expression, not shell expansion.

## Why `${HOME}` instead of `~`

A symlink target does not assign HOME semantics to `~`.

For example:

```text
link -> ~/source
```

is a relative symlink whose first component is literally `~`.

Using `~` as registry HOME syntax would therefore collide with ordinary symlink target text and with canonical target cleanup:

```text
./~/foo
    -> canonical target
~/foo
```

Using `${HOME}` keeps literal `~` available without adding an escape rule.

## Why not `$HOME`

Supporting both `$HOME` and `${HOME}` adds syntax without adding capability.

The braced token gives an explicit boundary and avoids questions such as:

```text
$HOMEfoo
$HOME-suffix
$USER
```

The registry therefore has exactly one predefined HOME expression.

## Expansion boundary

HOME expansion occurs while interpreting registry source and before field-specific validation.

Conceptually:

```text
registry source
    |
    v
expand_registry_home()
    |
    +-- link   -> link validation / normalization
    |
    +-- target -> target canonicalization
    |
    v
concrete validated Entry
```

The following layers do not carry HOME-expression state:

```text
planning
transactions
pending state
target comparison
fix
fix -f
recovery
symlink()
```

A serializer may later choose `${HOME}` spelling when writing the registry, but that is a source-format decision applied to a concrete `Entry`, not runtime target semantics.

## Link semantics

For registry `link`:

1. expand a leading HOME expression if present;
2. require the resulting path to be absolute;
3. apply the existing link normalization rules.

Duplicate detection and path identity checks operate on the concrete validated path.

## Target semantics

For registry `target`:

1. detect and expand a leading HOME expression before target canonicalization;
2. otherwise treat the value as ordinary pathname text;
3. apply the existing canonical target rules;
4. reject a canonical **relative** target whose spelling falls into the reserved HOME-expression namespace.

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
    -> literal relative target
```

## Reserved canonical relative target spellings

A literal relative symlink target can theoretically be:

```text
${HOME}/foo
```

If slink persisted that exact relative spelling in the registry, the next registry read would interpret it as a HOME expression.

The design intentionally does not add an escape language for this rare case.

The canonical relative target spellings:

```text
${HOME}
${HOME}/...
```

are therefore reserved as registry persistence spellings.

This is a registry encoding restriction, not a filesystem restriction.

The response depends on the source:

| Source | Behavior |
| --- | --- |
| hand-authored registry literal whose target canonicalizes into the reserved form | reject |
| `--relative` generation that produces the reserved form | reject |
| `adopt` observing the reserved form from `readlink()` | preserve meaning by registering an equivalent safe absolute target |

## Persistability invariant

Every validated or generated `Entry` that slink may write must be serializable under the registry's configured write style and readable again with the same target meaning.

Conceptually:

```text
concrete validated Entry
    |
    | configured serialization
    v
registry source
    |
    | parse + validate
    v
same concrete meaning
```

A read-only validation does not rewrite registry source.

For example:

```toml
target = "./${HOME}/foo"
```

is not a HOME expression at the input boundary.

Canonicalization produces:

```text
${HOME}/foo
```

The source file is still unchanged. The problem is that the validated `Entry` contains only the canonical target and no provenance saying it originally came from `./${HOME}/foo`.

Persisting that relative canonical spelling later would be ambiguous on the next load. Therefore validation rejects the literal registry input once its canonical form enters the reserved namespace.

The rejection establishes safe future persistence; it is not because validation itself immediately rewrites the file.

## Canonicalization order

Expression recognition happens before target canonicalization.

```text
"${HOME}/foo"
    -> HOME expression
    -> expand
    -> canonicalize concrete absolute target
```

But:

```text
"./${HOME}/foo"
    -> not an expression at the input boundary
    -> canonical target "${HOME}/foo"
    -> reserved relative spelling
    -> error
```

This keeps canonicalization deterministic and avoids context-sensitive exceptions such as preserving a leading `./` only before a HOME-looking token.

## Registry write preference

A registry may optionally choose how slink serializes HOME-based **absolute** paths when slink creates or updates an entry.

The proposed registry setting is:

```toml
[format]
home = "expression"
```

Supported values are:

```text
concrete
expression
```

If `[format]` or `format.home` is absent, the default is:

```text
concrete
```

The preference is registry-level, not a per-user environment or CLI preference. Source spelling belongs to the shared registry; two users should not cause the same registry to oscillate between concrete and expression spelling merely because their personal settings differ.

### `concrete`

slink-generated values use the concrete validated path:

```toml
[[link]]
link = "/Users/alice/.local/bin/tool"
target = "/Users/alice/src/tool"
```

Hand-authored HOME expressions are still accepted as input. If slink later rewrites that entry, the generated HOME-based absolute values use concrete spelling.

### `expression`

When slink creates or updates an entry, an absolute `link` or absolute `target` that is HOME itself or a descendant of HOME is serialized with the HOME token:

```toml
[format]
home = "expression"

[[link]]
link = "${HOME}/.local/bin/tool"
target = "${HOME}/src/tool"
```

The in-memory entry remains concrete:

```text
Entry.link   = /Users/alice/.local/bin/tool
Entry.target = /Users/alice/src/tool
```

Only registry serialization chooses the expression spelling.

### Relative targets are never rewritten by the HOME preference

The HOME write preference must not change target representation style.

For example:

```toml
[format]
home = "expression"

[[link]]
link = "${HOME}/project/bin/tool"
target = "../src/tool"
```

The relative target remains `../src/tool`.

It must not be converted to:

```toml
target = "${HOME}/project/src/tool"
```

because that would change a relative representation into an absolute representation and would interfere with the `--relative` / `-f` model from #28.

Therefore HOME-expression serialization applies only to absolute values.

### HOME containment is component-based

The serializer determines whether an absolute value is HOME or a descendant of HOME by path components, not by raw string prefix.

If HOME is:

```text
/Users/alice
```

then:

```text
/Users/alice
    -> ${HOME}

/Users/alice/src/tool
    -> ${HOME}/src/tool

/Users/alice2/src/tool
    -> not under HOME
    -> remain concrete
```

The serializer must not resolve target symlinks merely to decide source spelling.

### Embedded HOME-looking components remain literal

If the concrete absolute path is:

```text
/Users/alice/${HOME}/foo
```

expression serialization may produce:

```text
${HOME}/${HOME}/foo
```

On the next read, only the leading token is special, producing the original concrete path:

```text
/Users/alice/${HOME}/foo
```

This round-trips safely.

## No registry-wide normalization

The write preference applies only when slink creates or updates an entry.

It does **not** make the entire registry a canonical formatting surface.

For example, after a user adds:

```toml
[format]
home = "expression"
```

existing entries such as:

```toml
[[link]]
link = "/Users/alice/bin/old"
target = "/Users/alice/src/old"
```

remain unchanged until that entry itself is rewritten by slink.

Changing or writing another entry must not normalize the untouched entry.

This deliberately preserves the existing source-editing principle from #28: unrelated registry mutations do not become cleanup passes.

Users who want an existing registry fully converted to HOME expressions can perform a one-time manual replacement. The design does not add automatic whole-file normalization or a `slink format` command for this feature.

### Updated entry, not per-field provenance

The implementation does not need to remember which individual field was originally hand-authored or whether a source expression was used.

When slink creates or updates an entry, the values it writes for that entry follow the registry write preference. Other entries retain their source spelling.

This avoids per-field dirty/provenance state while keeping unrelated entries untouched.

## slink-generated output under each style

With HOME `/Users/alice`:

| Concrete validated value | `home = "concrete"` | `home = "expression"` |
| --- | --- | --- |
| link `/Users/alice/bin/x` | `/Users/alice/bin/x` | `${HOME}/bin/x` |
| target `/Users/alice/src/x` | `/Users/alice/src/x` | `${HOME}/src/x` |
| target `/opt/x` | `/opt/x` | `/opt/x` |
| target `../src/x` | `../src/x` | `../src/x` |
| target `~/src/x` | `~/src/x` | `~/src/x` |

The write preference never changes the concrete runtime meaning.

## HOME changes

A source HOME expression is evaluated during each fresh registry validation using the current HOME value.

Once an operation enters validated runtime state, HOME expression syntax is gone:

```text
registry source
    |
    | validation under current HOME
    v
concrete Entry
    |
    | transaction boundary
    v
concrete Pending state
```

Pending transactions contain concrete target text. Recovery therefore does not reinterpret a stored pending target according to a later HOME value.

If the registry uses `home = "expression"`, future fresh reads intentionally resolve those source expressions under the then-current HOME. That is source semantics, not transaction semantics.

## CLI behavior

This design does not add `${HOME}` parsing to CLI operands.

CLI operands keep the existing rules. Shell expansion and any existing CLI `~` handling remain separate from registry expression parsing.

## `adopt`

`readlink()` output is filesystem data, never registry expression syntax.

A literal target such as:

```text
~/foo
```

remains an ordinary relative target and may be adopted normally.

For a target whose canonical spelling falls into the reserved namespace:

```text
readlink() == "${HOME}/foo"

or

readlink() == "./${HOME}/foo"
    -> canonical target "${HOME}/foo"
```

`adopt` should preserve its meaning rather than reject the existing symlink.

It uses the existing link-aware reference interpretation to derive a safe absolute target:

```text
observed target
    |
    | canonical_target()
    v
canonical observed target
    |
    | reserved?
    +---- no  -> concrete target remains as observed canonical representation
    |
    +---- yes -> canonical_reference(link, observed target)
                 -> safe absolute target
```

The existing symlink is not rewritten.

The resulting concrete entry is then passed through the ordinary registry serializer:

- with `home = "concrete"`, the safe absolute target is written concretely;
- with `home = "expression"`, it may be shortened to a leading HOME expression if and only if that absolute target is under HOME.

For example, with HOME `/Users/alice` and an adopted target whose safe absolute meaning is:

```text
/Users/alice/work/${HOME}/foo
```

expression serialization may write:

```toml
target = "${HOME}/work/${HOME}/foo"
```

which reads back to the same concrete path.

Ordinary `fix` leaves the existing symlink untouched while it remains semantically correct. If the link is later missing, or if `fix -f` enforces the registered representation, slink materializes the concrete target represented by the registry source.

## `--relative`

Relative target generation follows the existing #28 algorithm and round-trip verification.

After producing the canonical relative candidate, slink checks the reserved namespace.

```text
candidate = "../${HOME}/foo"
    -> allowed

candidate = "${HOME}/foo"
    -> reserved
    -> error
```

The command fails rather than silently falling back to an absolute target. `--relative` explicitly requests a relative representation.

The HOME write preference does not change this result because it never converts relative targets.

## `fix`, `-f`, and semantic comparison

No HOME-expression state is required after validation.

Therefore:

- normal health uses the existing semantic target comparison;
- `fix` writes the concrete validated target for a missing link;
- `fix -f` enforces the concrete validated registered representation;
- an adopted reserved relative spelling may therefore be recreated or force-normalized as its safe absolute representation;
- transaction ownership and recovery retain exact target-text comparison.

Registry source spelling and symlink target spelling remain separate concerns.

## Transaction and recovery model

No HOME-specific expression state is added to pending operations.

Pending state contains the same concrete target representation used by #28.

The existing distinction remains:

```text
steady-state correctness
    -> semantic reference equality

transaction ownership / recovery
    -> exact concrete target-text equality
```

The registry write preference is not part of transaction identity.

## Implementation shape

The feature can stay at the registry/path boundary.

Conceptually:

```rust
enum HomeWriteStyle {
    Concrete,
    Expression,
}
```

Registry parsing expands source expressions:

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

Reserved relative spellings use one predicate:

```rust
fn is_reserved_home_target(target: &str) -> bool {
    target == "${HOME}" || target.starts_with("${HOME}/")
}
```

Registry target interpretation remains concrete:

```rust
fn registry_target(s: &str) -> Result<String> {
    if let Some(expanded) = expand_registry_home(s)? {
        return canonical_target(text(&expanded)?);
    }

    let target = canonical_target(s)?;

    if is_reserved_home_target(&target) {
        bail!("relative target conflicts with reserved registry HOME syntax");
    }

    Ok(target)
}
```

Serialization is separate:

```rust
fn serialize_home_absolute(path: &Path, style: HomeWriteStyle) -> Result<String> {
    if style == HomeWriteStyle::Expression {
        let home = home()?;

        if path == home {
            return Ok("${HOME}".to_owned());
        }

        if let Ok(rest) = path.strip_prefix(&home) {
            return Ok(format!("${HOME}/{}", text(rest)?));
        }
    }

    Ok(text(path)?.to_owned())
}
```

The production implementation should handle empty remainders and separators through path components rather than relying on the illustrative string formatting above.

For `target`, the serializer is called only when the concrete target is absolute. Relative targets are returned unchanged.

The registry document writer applies this policy only to entries it creates or updates. It clones and preserves source text for unrelated entries as it does today.

Adoption continues to use the shared reserved-form predicate but resolves the exceptional observed relative spelling through the existing link-aware reference helper before serialization.

## Validation and serialization matrix

| Source / operation | Concrete runtime value | Generated source under `concrete` | Generated source under `expression` |
| --- | --- | --- | --- |
| registry `link = "${HOME}/bin/x"` | `/Users/alice/bin/x` | only if rewritten: `/Users/alice/bin/x` | only if rewritten: `${HOME}/bin/x` |
| registry `target = "${HOME}/src/x"` | `/Users/alice/src/x` | only if rewritten: `/Users/alice/src/x` | only if rewritten: `${HOME}/src/x` |
| create absolute target under HOME | absolute | absolute | HOME expression |
| create target outside HOME | absolute | absolute | absolute |
| `--relative` target `../src/x` | relative | relative | relative |
| literal target `~/src/x` | relative | relative | relative |
| registry `target = "./${HOME}/x"` | rejected | n/a | n/a |
| adopt raw `${HOME}/x` | safe absolute reference | safe absolute | HOME expression only if safe absolute is under HOME |
| update another entry | unchanged entry stays source-identical | unchanged | unchanged |

## Errors

Errors should describe representation conflicts rather than claim the underlying pathname is invalid.

Example:

```text
relative target "${HOME}/foo" conflicts with reserved registry HOME syntax
```

This applies to explicit unpersistable relative representations such as hand-authored literal registry input or `--relative` output.

`adopt` instead re-encodes an observed reserved relative target as a safe absolute representation.

Invalid `format.home` values should report the accepted values:

```text
format.home must be "concrete" or "expression"
```

## Compatibility

The default write style is `concrete`, so registries that do not opt into `[format]` keep the existing slink-generated persistence behavior.

No per-entry schema migration is required.

A new slink version is required for:

- leading `${HOME}` expression semantics;
- the optional `[format]` table.

Older slink versions reject the new top-level `format` field under the current strict registry schema, so a registry using `[format]` intentionally requires the new version.

There is also one syntax-compatibility boundary: an old version treats a leading `${HOME}` target string as literal pathname text, whereas this design treats it as the HOME expression.

Literal `~` targets remain unchanged.

## Alternatives considered

### Use `~` for HOME

Rejected because it collides directly with real relative symlink target text and canonicalization.

### Use `$HOME`

Rejected because it adds no capability beyond the braced token while introducing less explicit boundaries and stronger shell-variable expectations.

### Support both `$HOME` and `${HOME}`

Rejected as unnecessary syntax surface.

### Add an escape syntax

Rejected because canonical target normalization would need context-sensitive exceptions.

### Carry expressions in runtime state

Rejected because it would split expression from materialized target and complicate planning, fixing, transactions, and recovery.

### Normalize the entire registry when write style changes

Rejected for this feature.

Existing absolute HOME paths are easy to replace manually if a user wants a one-time conversion. Automatic normalization would add unrelated diffs, require rules for when cleanup occurs, and weaken the existing principle that unrelated registry entries retain their source spelling.

The write preference therefore governs future slink-generated entry writes only.

### Use typed TOML expressions

For example:

```toml
target = { home = "src/foo" }
```

This completely separates literal strings from expressions and can represent every ordinary string without reserving a namespace.

It is not chosen because the additional schema and serialization surface is disproportionate to this narrow HOME convenience.

If exact representation of every theoretically valid relative target becomes a hard requirement, typed syntax remains the clean fallback.

## Focused tests

At minimum, implementation should cover:

1. registry `link = "${HOME}/..."` expands and validates;
2. registry `target = "${HOME}/..."` expands to a concrete absolute target;
3. `target = "~/..."` remains literal;
4. `target = "$HOME/..."` remains literal;
5. embedded `${HOME}` remains literal;
6. canonical `./${HOME}/...` is rejected as reserved;
7. absent `format.home` defaults to `concrete`;
8. explicit `format.home = "concrete"` writes HOME-based absolute values concretely;
9. `format.home = "expression"` writes newly created or updated HOME-based absolute `link` and `target` values with `${HOME}`;
10. expression mode leaves relative targets unchanged;
11. expression mode leaves absolute paths outside HOME concrete;
12. HOME containment is component-based, so a sibling such as `/Users/alice2` is not shortened when HOME is `/Users/alice`;
13. updating one entry does not rewrite another entry merely to apply the HOME write preference;
14. changing the write preference does not automatically normalize existing entries;
15. updating an entry serializes that entry according to the current registry write preference without adding per-field provenance state;
16. `adopt` preserves literal `~/...`;
17. `adopt` converts an observed reserved target to a semantically equivalent safe absolute target without rewriting the existing symlink;
18. adopt conversion remains correct through symlink parents by reusing the existing link-aware reference semantics;
19. expression mode may serialize the safe adopted absolute target with a leading HOME expression when it lies under HOME;
20. relative generation rejects a candidate beginning with `${HOME}/...`;
21. relative generation allows `../${HOME}/...`;
22. pending transaction data contains only the concrete materialized target;
23. recovery remains independent of later HOME changes.

## Final contract

The feature can be summarized as:

> Registry `link` and `target` values may use `${HOME}` or `${HOME}/...` as a HOME-relative source expression. Validation always produces concrete runtime entries. The registry may choose `format.home = "concrete"` (the default) or `"expression"` for slink-generated writes of newly created or updated entries; expression mode shortens only absolute values that are HOME or descendants of HOME, never relative targets. Unrelated existing entries retain their source spelling and are not normalized automatically. Canonical relative target spellings beginning with `${HOME}` remain reserved: explicit registry literals and `--relative` generation reject that representation, while `adopt` preserves an existing symlink's meaning by registering a safe absolute target before ordinary serialization.

This keeps HOME semantics at the registry boundary, keeps runtime and transaction state concrete, and makes source spelling a small registry-level write preference rather than a second target model.
