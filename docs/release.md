# Making releases

Decision 0011 says why; this is how.

## Day to day

Commit with conventional commits (`feat:`, `fix:`, `chore:` ...). release-plz keeps a PR "chore: release
v<next>" open on the default branch with the version bump and `CHANGELOG.md`. Merging it releases: GitHub gets
the tag `v<version>` and a draft release, the package
jobs attach the installers, the Linux tarball, the update signatures and attestations, and the last job writes
`latest.json` and publishes the release.

Everything runs on the workflow's own `GITHUB_TOKEN`. A pull request opened with it starts its `pull_request`
runs only after someone approves them on the PR page, so the release job dispatches CI on the release PR's
branch itself (a dispatch needs no approval); its checks land on the PR's head commit. Approving the waiting runs
as well only runs CI twice.

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

crates.io publishing is off. To turn it on for `wavedude-core` and `wavedude-export`: in `release-plz.toml`
replace their `release = false` with `version_group = "wavedude"` (and give the app the same group), add a
crates.io token as the secret `CARGO_REGISTRY_TOKEN` for the first publish, then add the repository as a
trusted publisher of both crates (crates.io > crate > Settings > Trusted Publishing, workflow `release.yml`) and
delete the secret.

- Settings > General: squash merging only, its commit titled by the pull request's title (release-plz reads
  conventional commits); allow auto-merge; delete head branches; no wiki.
- Settings > Rules: a ruleset on the default branch requiring the CI jobs (`ubuntu-latest`, `windows-latest`,
  `macos-latest`, `MSRV`) and refusing deletion and force pushes; a ruleset on `v*` tags refusing update and deletion
  (creation stays open: a personal repository cannot let GitHub Actions bypass a rule, and release-plz tags through it).
  Administrators bypass both.
- Settings > Actions > General: actions pinned to a full commit hash; approval before running workflows from every
  outside contributor; the workflow token read-only by default, allowed to create pull requests (release-plz's).
- Settings > Advanced Security: Dependabot alerts and security updates, secret scanning with push protection, private
  vulnerability reporting (`SECURITY.md`).

## By hand

```sh
NO_STRIP=true cargo packager --release -p wavedude-app --formats appimage   # or nsis, app,dmg
```

The packages land in `target/packages`. Without `WAVEDUDE_UPDATE_ENDPOINT` and `WAVEDUDE_UPDATE_PUBKEY` in the
environment the build has no updater.
