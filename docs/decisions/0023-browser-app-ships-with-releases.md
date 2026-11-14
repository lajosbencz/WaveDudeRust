# 0023: The browser app ships with releases

Status: accepted, 2026-11-14 (the user's decision). Amends [0019](0019-egui-and-the-platform-layer.md): GitHub Pages
serves the latest release's browser app, not master's.

## Context

[`pages.yml`](../../.github/workflows/pages.yml) built the browser app from every master commit that changed the code, so the page ran
unreleased work while the download link next to it pointed at the latest release. The two drifted, and the page's
build had none of the release's provenance.

## Decision

- [`package.yml`](../../.github/workflows/package.yml) builds the browser app of each release with
  [`tools/build-web.sh`](../../tools/build-web.sh) into `wavedude-rust-<version>-web.tar.gz`, attests it with its SBOM, and the release
  attaches it beside the desktop packages.
- [`pages.yml`](../../.github/workflows/pages.yml) downloads that bundle from the latest release, verifies its attestation against
  `package.yml`, and deploys it under `app/` with `site/`. It runs when `site/` changes on master, when
  [`release.yml`](../../.github/workflows/release.yml) dispatches it after publishing a release, and by hand.

## Consequences

- The page and the downloads are the same version.
- A code change reaches the page only through a release.
- The page cannot deploy before a release carries the bundle; the workflow fails until then.
