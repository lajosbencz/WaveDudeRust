# 0011 Releases, installers, dependency updates and the updater

Status: accepted, 2026-11-07

- **Versions** from conventional commits by [release-plz](https://github.com/release-plz/release-plz)
  (`release-plz.toml`): one version for the workspace, a release PR kept open on the default branch, and on its
  merge the app tagged `v<version>` with a draft GitHub release. `wavedude-core` and `wavedude-export` are ready
  for crates.io (the name `wave-core` is taken there, hence the `wavedude-` prefix on every crate) but not
  published yet, the user's choice; `docs/release.md` says how to turn it on. The other crates are
  `publish = false`.
- **Installers** by [cargo-packager](https://github.com/crabnebula-dev/cargo-packager) in the same workflow
  (`.github/workflows/release.yml`): NSIS on Windows x86_64, app and DMG on macOS (arm64 on `macos-15`, x86_64 on
  `macos-15-intel`), AppImage plus a tarball of both programs on Linux x86_64 (built on Ubuntu 22.04 for an older
  glibc). Both programs are in every package. Not signed by Apple or Microsoft (macOS bundles carry an ad-hoc
  signature, without which Apple silicon reports them as damaged): Gatekeeper and SmartScreen warn,
  accepted. Each file gets a GitHub build provenance attestation instead (Sigstore; free, verifiable with
  `gh attestation verify`, but no operating system checks it). The release turns public only after every
  platform's files and `latest.json` are attached.
- **Updater**: [cargo-packager-updater](https://crates.io/crates/cargo-packager-updater) against
  `releases/latest/download/latest.json`, whose packages are signed with a minisign key (the private key a
  secret, the public key a repository variable compiled in through `WAVEDUDE_UPDATE_PUBKEY`); the endpoint is
  derived from the repository the workflow runs in. A build without both has no updater, no menu entries and no
  startup question. The check runs at launch only if the user agreed on the startup page; installing asks
  each time. It replaces the NSIS install, the macOS app and an AppImage; a plain Linux binary is told where
  the release is.
- **Dependencies**: Dependabot for Cargo and for the workflows' actions, which are pinned to commit hashes with
  the version in a comment (Dependabot moves both). Its security updates come as soon as an advisory appears;
  patch and minor updates, security ones included, merge on their own once CI passes
  (`dependabot-automerge.yml`), major ones wait for a person (telling a major security update apart needs
  fetch-metadata's alert lookup, which needs a personal token). Renovate was the alternative; Dependabot needs no app installed.
- **Forks**: CI runs anywhere with no secrets. The release workflow is opt-in through the repository variable
  `RELEASE_ENABLED`; it uses the fork's own repository for tags, releases and the update feed, and the fork's
  own keys (crates.io publishing needs crate names the fork owns).
