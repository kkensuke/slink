# Homebrew releases

Homebrew distribution uses two repositories:

- `kkensuke/slink` owns source code, versions, tags, tests, GitHub Releases, and the release workflow.
- `kkensuke/homebrew-tap` owns the generated `Formula/slink.rb` consumed by Homebrew.

Users install slink with:

```sh
brew install kkensuke/tap/slink
```

## One-time setup

The existing `kkensuke/homebrew-tap` repository is reused. The release workflow in this repository needs permission to push only the generated Formula to that Tap.

### 1. Create or reuse a fine-grained PAT

Use a fine-grained personal access token with these permissions:

| Setting | Value |
| --- | --- |
| Resource owner | `kkensuke` |
| Repository access | Only select repositories |
| Selected repository | `homebrew-tap` |
| Repository permissions → Contents | Read and write |

No write access to `slink` or other repositories is required for this PAT.

If the existing token used by another release workflow already has exactly this access and is still valid, it can be reused.

### 2. Store the PAT in the `release` Environment

In `kkensuke/slink`, create a GitHub Actions Environment named `release` and add an Environment secret:

```text
Name: HOMEBREW_TAP_TOKEN
Value: <fine-grained PAT>
```

The workflow reads this credential through `${{ secrets.HOMEBREW_TAP_TOKEN }}` when checking out and pushing `kkensuke/homebrew-tap`.

Optional required reviewers on the `release` Environment can be used as a manual gate before publishing a release.

## Publishing a release

The release workflow is triggered by a `vMAJOR.MINOR.PATCH` tag.

### 1. Set the package version

Update `Cargo.toml`:

```toml
[package]
version = "0.1.0"
```

Commit the version change and merge it to `main`. The release workflow rejects a tag whose version does not match `Cargo.toml`, or whose commit is not contained in `main`.

### 2. Create and push the release tag

From an up-to-date, clean `main` branch:

```sh
git switch main
git pull --ff-only
git status --short

VERSION=0.1.0
git tag -s "v${VERSION}" -m "slink ${VERSION}"
git tag -v "v${VERSION}"
git push origin "v${VERSION}"
```

Signed tags are the project release procedure. If signing is not configured yet, configure GPG locally before creating the release tag.

### 3. What the tag triggers

The workflow performs this sequence:

```text
push vMAJOR.MINOR.PATCH
        ↓
verify tagged commit is on main
        ↓
verify tag version == Cargo.toml version
        ↓
fmt / clippy / tests / APFS tests / release build on macOS
        ↓
create GitHub Release with generated notes
        ↓
download and hash the GitHub tag archive
        ↓
render Formula/slink.rb
        ↓
commit and push Formula/slink.rb to kkensuke/homebrew-tap
```

The Formula builds slink from the tagged source with Homebrew's Rust build dependency. No prebuilt release binary is required by Homebrew.

The workflow is retry-friendly: it skips GitHub Release creation if the release already exists, and only commits the Formula when its generated content changed.

## Verifying a release

After the release workflow succeeds, confirm that `kkensuke/homebrew-tap/Formula/slink.rb` exists and then test a clean Homebrew installation:

```sh
brew update
brew uninstall slink 2>/dev/null || true
brew install kkensuke/tap/slink
slink --version
brew test kkensuke/tap/slink
```

For later versions, the normal upgrade path is:

```sh
brew update
brew upgrade slink
```

## If publishing to the Tap fails

Check these first:

- the `release` Environment exists in `kkensuke/slink`;
- `HOMEBREW_TAP_TOKEN` is stored as an Environment secret, not a variable;
- the token is not expired;
- the token can access `kkensuke/homebrew-tap`;
- the token has `Contents: Read and write` permission.

After correcting the setup, rerun the failed GitHub Actions jobs for the existing tag. Do not move a published release tag only to retry the workflow.
