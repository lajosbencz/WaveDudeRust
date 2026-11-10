# Making releases

Decision 0011 says why; this is how.

## Day to day

Commit with conventional commits (`feat:`, `fix:`, `chore:` ...). After each push to the default branch,
git-cliff (`cliff.toml`) reads the commits since the last `v*` tag. If any ships something (`feat`, `fix`, `perf`,
`refactor`, `revert`, dependency updates as `fix(deps)` or `chore(deps)`; not `ci`, `docs`, `test`, `style`, `build`
or other `chore`), the workflow keeps one pull request "chore(release): v<next>" open on the branch `release-pr`: the
workspace version bumped, `Cargo.lock`, and `CHANGELOG.md` regenerated from all tags, its new section as the PR's
body. In 0.x a feature bumps the patch and a breaking change (`!`) the minor. The branch is rebuilt as one commit on
every push, so edits to it are lost; change the commits instead (or the version in the PR, by hand, just before
merging).

Merging it (squash) releases: the push finds the workspace version without its tag, tags the commit `v<version>` and
drafts the release with the changelog section as notes. `package.yml`, a reusable workflow without secrets and with
a read-only token, builds the packages with `cargo auditable` and attests them (build provenance and the CycloneDX
SBOMs of both programs); `sign` signs the update bundles with the updater key without compiling anything; `publish`
writes `latest.json` and `SHA256SUMS`, attests them and the signatures, attaches everything, verifies every
attached file against its attestation (a file not attested by `package.yml` or `release.yml` of this repository
stops the release), appends the provenance table to the notes (file, digest, workflow) and publishes the release,
which the repository's immutable releases then lock; GitHub's own release attestation follows, checked with
`gh release verify-asset`.

Everything runs on the workflow's own `GITHUB_TOKEN`. A pull request opened with it starts its CI only after someone
approves the waiting runs on the PR page ("Approve workflows to run"); once approved, later updates of the same PR
run without asking. The required checks come from those runs alone, so each new release PR needs that one approval.

A failed release is finished by rerunning the failed jobs of its run: tagging and drafting skip what exists. The
release stays a draft until `publish` succeeds.

## One-time setup of a repository

Settings > Secrets and variables > Actions:

| Kind | Name | Value |
|---|---|---|
| variable | `RELEASE_ENABLED` | `true` (without it the release workflow does nothing) |
| variable | `UPDATER_PUBKEY` | the updater's public key (below); unset: builds without the updater |
| secret | `UPDATER_PRIVATE_KEY` | the updater's private key |
| secret | `UPDATER_PRIVATE_KEY_PASSWORD` | its password |

The updater key, made once and kept safe (losing it means installed copies cannot update to newer releases):

```sh
cargo install cargo-packager --locked
cargo packager signer generate --path wavedude-updater.key
```

The `.pub` file's content is `UPDATER_PUBKEY`, the key file's content `UPDATER_PRIVATE_KEY`.

crates.io publishing is off. `wavedude-core` and `wavedude-export` are ready for it (`wavedude-export` names
`wavedude-core`'s version, which the release PR keeps in step); turning it on means a job in `release.yml` after
`release` running `cargo publish -p wavedude-core` then `-p wavedude-export`, with the repository as their trusted
publisher (crates.io > crate > Settings > Trusted Publishing; a token only for each crate's first publish).

- Settings > General: immutable releases on (`gh api -X PUT repos/<owner>/<repo>/immutable-releases`); squash
  merging only, its commit titled by the pull request's title (git-cliff reads conventional commits); allow
  auto-merge; delete head branches; no wiki.
- Settings > Rules: a ruleset on the default branch requiring the CI jobs (`ubuntu-latest`, `windows-latest`,
  `macos-latest`, `MSRV`) and refusing deletion and force pushes; a ruleset on `v*` tags refusing update and deletion
  (creation stays open: a personal repository cannot let GitHub Actions bypass a rule, and the release job tags
  through it).
  Administrators bypass both.
- Settings > Actions > General: actions pinned to a full commit hash; approval before running workflows from every
  outside contributor; the workflow token read-only by default, allowed to create pull requests (the release PR).
- Settings > Advanced Security: Dependabot alerts and security updates, secret scanning with push protection, private
  vulnerability reporting (`SECURITY.md`).

## By hand

```sh
NO_STRIP=true cargo packager --release -p wavedude-app --formats appimage   # or nsis, app,dmg
```

The packages land in `target/packages`. Without `WAVEDUDE_UPDATE_ENDPOINT` and `WAVEDUDE_UPDATE_PUBKEY` in the
environment the build has no updater.
