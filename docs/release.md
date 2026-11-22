# Making releases

Decision 0011 says why; this is how.

## Day to day

Commit with conventional commits (`feat:`, `fix:`, `chore:` ...). After each merge to the default branch,
git-cliff ([`cliff.toml`](../cliff.toml)) reads the commits since the last `v*` tag. If any ships something (`feat`, `fix`, `perf`,
`refactor`, `revert`, dependency updates as `fix(deps)` or `chore(deps)`; not `ci`, `docs`, `test`, `style`, `build`
or other `chore`), the workflow keeps one pull request "chore(release): v<next>" open on the branch `release-pr`: the
workspace version bumped, [`Cargo.lock`](../Cargo.lock), and [`CHANGELOG.md`](../CHANGELOG.md) regenerated from all tags, its new section as the PR's
body. In 0.x a feature bumps the patch and a breaking change (`!`) the minor. The branch is rebuilt as one commit on
every push, so edits to it are lost; change the commits instead (or the version in the PR, by hand, just before
merging).

Merging it (squash) releases: the push finds the workspace version without its tag, tags the commit `v<version>` and
drafts the release with the changelog section as notes. [`package.yml`](../.github/workflows/package.yml), a reusable workflow without secrets and with
a read-only token, builds the packages with `cargo auditable` (the Linux ones in an AlmaLinux 8 container, checked to
need glibc 2.28 at most) and attests them (build provenance and the CycloneDX
SBOMs of both programs) and the browser app, `WaveDude_Rust-<version>-web.tar.gz`; `sign` waits for a maintainer to approve the `release` environment, then signs the update bundles with the updater key and the cargo-packager that `signer` built in a job of its own, compiling nothing; `publish`
writes `latest.json` and `SHA256SUMS`, attests them and the signatures, attaches everything, verifies every
attached file against its attestation (a file not attested by [`package.yml`](../.github/workflows/package.yml) or [`release.yml`](../.github/workflows/release.yml) of this repository
stops the release), appends the provenance table to the notes (file, digest, workflow), publishes the release and dispatches
[`pages.yml`](../.github/workflows/pages.yml), which deploys its browser app,
which the repository's immutable releases then lock; GitHub's own release attestation follows, checked with
`gh release verify-asset`.

Everything runs on the workflow's own `GITHUB_TOKEN`, whose pushes and pull requests start no workflow; after each
rebuild of the release PR the workflow dispatches CI and hygiene on its branch, which report the required checks on
its head commit.

A failed release is finished by rerunning the failed jobs of its run: tagging and drafting skip what exists. The
release stays a draft until `publish` succeeds.

## One-time setup of a repository

Settings > Secrets and variables > Actions holds the variables; the secrets belong to the environment `release`
(Settings > Environments), which admits the default branch alone and waits for a maintainer's approval, so only the
`sign` job of a release on the default branch reads them:

| Kind | Name | Value |
|---|---|---|
| variable | `RELEASE_ENABLED` | `true` (without it the release workflow does nothing) |
| variable | `UPDATER_PUBKEY` | the updater's public key (below); unset: builds without the updater |
| `release` secret | `UPDATER_PRIVATE_KEY` | the updater's private key |
| `release` secret | `UPDATER_PRIVATE_KEY_PASSWORD` | its password |

The updater key, made once and kept safe (losing it means installed copies cannot update to newer releases):

```sh
cargo install cargo-packager --locked
cargo packager signer generate --path wavedude-updater.key
```

The `.pub` file's content is `UPDATER_PUBKEY`, the key file's content `UPDATER_PRIVATE_KEY`.

crates.io publishing is off: every crate is `publish = false`. The library's API is not ready
([decision 0031](decisions/0031-modular-core-and-frontends.md)); once it is, turning publishing on means dropping `publish = false` and a job in
[`release.yml`](../.github/workflows/release.yml) after `release` running `cargo publish -p waveduderust_macros`, then `-p waveduderust`,
in that order, with the repository as their trusted
publisher (crates.io > crate > Settings > Trusted Publishing; a token only for each crate's first publish).

- Settings > Pages: source GitHub Actions; [`pages.yml`](../.github/workflows/pages.yml) deploys `site/` from the default
  branch with the latest release's browser app ([decision 0023](decisions/0023-browser-app-ships-with-releases.md)). The environment `github-pages` (Settings > Environments, created by the first deployment) admits the
  default branch alone.
- Settings > General: immutable releases on (`gh api -X PUT repos/<owner>/<repo>/immutable-releases`); squash
  merging only, its commit titled by the pull request's title (git-cliff reads conventional commits); allow
  auto-merge; delete head branches; no wiki.
- Settings > Rules: a ruleset on the default branch requiring a pull request, the checks `CI result` and
  `Hygiene result` (each its workflow's jobs combined, so a job added there needs no ruleset change) on a branch up
  to date with it, and refusing deletion and force pushes. Administrators bypass it to push docs directly, which only the hygiene checks
  run on. A merge queue needs an organization-owned repository. A ruleset on `v*` tags refusing
  update and deletion (creation stays open: a personal repository cannot let GitHub Actions bypass a rule, and the
  release job tags through it); administrators bypass it.
- Settings > Actions > General: actions pinned to a full commit hash; approval before running workflows from every
  outside contributor; the workflow token read-only by default, allowed to create pull requests (the release PR).
- Settings > Advanced Security: Dependabot alerts and security updates, secret scanning with push protection, private
  vulnerability reporting ([`SECURITY.md`](../SECURITY.md)).

## By hand

On Windows and macOS:

```sh
cargo build --release -p waveduderust_classic -p waveduderust_cli
cargo packager --release -p waveduderust_classic --formats nsis   # or app,dmg
```

On Linux, in the release's build container ([`.github/build/linux.Dockerfile`](../.github/build/linux.Dockerfile), [decision 0011](decisions/0011-releases-and-updates.md)),
whose libraries the AppImage carries, with docker or podman; a target directory of its own keeps out what the host
compiled against a newer glibc:

```sh
docker build -t wavedude-build - < .github/build/linux.Dockerfile
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:$PWD" -w "$PWD" -e CARGO_TARGET_DIR=target/linux \
  -e NO_STRIP=true -e APPIMAGE_EXTRACT_AND_RUN=1 wavedude-build sh -ec '
    rustup toolchain install
    cargo install --locked cargo-packager@0.11.8
    cargo build --release -p waveduderust_classic -p waveduderust_cli
    cargo packager --release -p waveduderust_classic --formats appimage
    .github/build/glibc-floor.sh target/linux/release/wavedude-app target/linux/release/wavedude-cli target/packages/*.AppImage'
```

Rootless podman takes `--userns=keep-id` in place of `--user`, so the files stay the caller's. The floor check prints
the highest glibc symbol version and fails above 2.28, naming the file and the symbol.

The packages land in `target/packages`. Without `WAVEDUDE_UPDATE_ENDPOINT` and `WAVEDUDE_UPDATE_PUBKEY` in the
environment the build has no updater.

## Icons

[`frontends/classic/assets/icon.svg`](../frontends/classic/assets/icon.svg) is the master; [`assets/icons/`](../frontends/classic/assets/icons/) holds what is rendered from it, committed beside
it: the PNG sizes 16 to 512 (the Linux icon theme, the window and the About dialog), [`icon.ico`](../frontends/classic/assets/icons/icon.ico) with 16 to 256 (the
Windows executable through [`build.rs`](../frontends/classic/build.rs), and the installers) and [`icon.icns`](../frontends/classic/assets/icons/icon.icns) with 16 to 512 at 1x and 2x (macOS).
cargo-packager reads them from the source tree, so the build cannot make them: after a change to the SVG, render the
set again at those sizes and commit it with the SVG.

## File types and desktop integration

`.wdproj` and `.wdscheme` are declared once per platform, all naming the MIME types of the file associations in
[`frontends/classic/Cargo.toml`](../frontends/classic/Cargo.toml):

- Windows: the NSIS installer offers the ProgIds `WaveDudeRust.Project` and `WaveDudeRust.ColorScheme` (the
  associations' `name`) as an unticked component, ticked when an earlier install registered them: an unsigned
  installer that changes file associations unasked is flagged by Defender. The template is cargo-packager 0.11.8's
  with that change, [`assets/windows/installer.nsi`](../frontends/classic/assets/windows/installer.nsi); its
  uninstaller removes the associations still pointing at the app (the stock one loops over `association.ext`, a
  field that does not exist). A cargo-packager bump means diffing the template again.
- macOS: [`assets/macos/Info.plist`](../frontends/classic/assets/macos/Info.plist), merged over cargo-packager's, exports the two types and names them in the document
  types. The Finder hands a file to the running app as `application:openURLs:`, which the app delegate of
  [`open_requests.rs`](../crates/waveduderust_app/src/open_requests.rs) queues for the window.
- Linux: [`assets/linux/wavedude-rust.xml`](../frontends/classic/assets/linux/wavedude-rust.xml) (shared-mime-info) maps the globs to the MIME types the `.desktop` file names;
  the AppImage carries it, and the tarball carries it with [`wavedude-app.desktop`](../frontends/classic/assets/linux/wavedude-app.desktop), the icon theme PNGs and [`install.sh`](../frontends/classic/assets/linux/install.sh),
  which installs them under `PREFIX` (default `~/.local`) and removes them with `--uninstall`.
