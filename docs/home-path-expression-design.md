# HOME path expression design

## Status

Design proposal.

This document defines a minimal HOME-directory expression for registry `link` and `target` values while preserving the relative-target model introduced by the relative symlink target work.

## Goals

- Allow users to write HOME-relative paths in the registry.
- Keep `Entry.link` and `Entry.target` concrete after validation.
- Preserve literal `~` symlink targets.
- Avoid a general environment-variable or template language.
- Avoid new persistent fields, transaction variants, or recovery state.

## Proposed syntax

Only these registry-source forms are special:

```text
${HOME}
${HOME}/...
```

All other forms, including `~`, `~/...`, `$HOME`, `${USER}`, and embedded `${HOME}`, are ordinary pathname text.

Expansion happens once while validating registry source. Validated entries contain only concrete canonical paths or target text.

slink-generated registry values never emit `${HOME}`; the expression is accepted as hand-authored input syntax only.

## Core invariant

After validation:

```text
link   = concrete managed symlink location
target = concrete canonical target text slink would materialize
```

HOME expressions do not survive into planning, transactions, recovery, fixing, or target comparison.

## Follow-up sections

The complete design will specify reserved relative-target forms, adoption behavior, `--relative` interaction, persistence rules, compatibility, errors, and focused tests.
