# Contributing

- Open an issue before a new feature or a design change; a design accepted there gets a record in `docs/decisions`.
- The ground rules in `docs/SEED.md` apply to every change.
- Pull requests are squash-merged and the title becomes the commit: write it as a
  [conventional commit](https://www.conventionalcommits.org/); release-plz builds the changelog from it.
- The checks in `.github/workflows/ci.yml` must pass, on the workspace's `rust-version` too.
- Snapshot changes (`insta`) are reviewed with `cargo insta review` and explained in the pull request.
- A behaviour change updates its spec (`docs/spec`, `docs/ui`) and, if users see it, `crates/wavedude-app/assets/manual.md`.
- A new UI string goes into all four tables in `crates/wavedude-app-state/assets/strings`.
- Contributions are licensed GPL-3.0-or-later; vulnerabilities go through `SECURITY.md`.
