# Contributing

Read first: the ground rules in [`docs/SEED.md`](docs/SEED.md), the conventions the code follows in [`docs/conventions.md`](docs/conventions.md), and
what differs from WaveDude Next and why in [`docs/divergences.md`](docs/divergences.md). [`docs/README.md`](docs/README.md) maps the documentation.

## Issues

Search the open and closed issues first. The forms under [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE/) ask for what each kind needs:

- **Bug**: the version (Help > About, or `wavedude-cli --version`), the platform, the steps, what was expected,
  the project file or the name of the example used, and for a wrong picture or export the output itself.
- **Crash**: the same plus the log of the day, which holds the panic's backtrace. The logs folder is
  `~/.config/waveduderust/logs` on Linux, `~/Library/Application Support/WaveDudeRust/logs` on macOS and
  `%APPDATA%\WaveDudeRust\config\logs` on Windows.
- **Feature request**: the problem, not a solution; whether WaveDude Next has the feature (then it is an
  alignment, say how Next behaves); how it fits the goals of [`docs/SEED.md`](docs/SEED.md) (a drop-in for WaveDude Next users,
  physics per the literature, one behaviour on three platforms). A feature Next has is accepted; a new one is
  accepted when it keeps `.wdproj` files readable by Next and does not trade accuracy for anything. A request
  nobody plans to build within a year is closed, not parked.
- **Differs from WaveDude Next**: check [`docs/divergences.md`](docs/divergences.md) first; a listed difference is on purpose. Otherwise
  report what each program does, the project file, and the section of [`docs/spec`](docs/spec/) or [`docs/ui`](docs/ui/) that says what
  should happen. When Next is the one in the wrong, this program does not imitate the bug; the maintainers decide
  whether and how to pass it on.
- Vulnerabilities go through [`SECURITY.md`](SECURITY.md), never an issue.
- Conduct: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Pull requests

- Open an issue before a feature or a design change; a design accepted there gets a record in [`docs/decisions`](docs/decisions/).
  A fix for an open issue needs no discussion.
- One concern per pull request, no unrelated commits. Pull requests are squash-merged and the title becomes the
  commit: write it as a conventional commit (below). The body says what changed, why, and how it was verified.
- Accepted when all of this holds:
  - The checks of [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (on Linux, macOS and Windows) and
    [`.github/workflows/hygiene.yml`](.github/workflows/hygiene.yml) pass; Tools, below, lists them with the commands to run them locally.
  - A behaviour change comes with a test that failed before it: unit, headless (freya-testing), physics or
    snapshot, whichever fits ([`docs/conventions.md`](docs/conventions.md) says where each lives). Snapshot changes (`insta`) are
    reviewed with `cargo insta review` and explained in the pull request.
  - The specification follows the behaviour: [`docs/spec`](docs/spec/) for the core and export, [`docs/ui`](docs/ui/) for the application,
    [`crates/wavedude-app/assets/manual.md`](crates/wavedude-app/assets/manual.md) for what users see. A new UI string goes into all four tables in
    [`crates/wavedude-app-state/assets/strings`](crates/wavedude-app-state/assets/strings/).
  - A difference from WaveDude Next is warranted by the criteria in [`docs/divergences.md`](docs/divergences.md) and recorded there,
    with its reasoning in a decision record.
  - Physics is correct per the literature, cited in [`docs/physics.md`](docs/physics.md), with a test that states its tolerance;
    nothing is calibrated to WaveDude Next's output.
  - A performance claim comes with its measurement in [`docs/measurements.md`](docs/measurements.md), including the command.
  - A new dependency is named in the pull request with the reason; the lockfile change is part of the same
    commit.
  - The code follows [`docs/conventions.md`](docs/conventions.md); comments state the present, not the history; ASCII only.
  - Markdown names a file or folder of the repository as a relative link (`` [`docs/physics.md`](docs/physics.md) ``),
    never as bare code; the app renders `manual.md`, so it links nothing of the repository.
- Reviewers may push fixups onto the branch. A pull request that goes quiet for a month is closed and can be
  reopened.

## Tools

The compiler is pinned in [`rust-toolchain.toml`](rust-toolchain.toml) with clippy and rustfmt; rustup installs it on the
first `cargo` call. Bump it there when a new stable is wanted (the CI caches are keyed on it). Linux needs the
packages of the "Linux packages for Skia and winit" step of [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Below,
`F=wavedude-export/openh264-source,wavedude-cli/openh264-source`.

| Tool | Checks | Run | Configured in |
|---|---|---|---|
| rustfmt | formatting | `cargo fmt --all` | [`rustfmt.toml`](rustfmt.toml) |
| clippy | lints, warnings fail | `cargo clippy --workspace --all-targets --features $F -- -D warnings` | `[workspace.lints]` of [`Cargo.toml`](Cargo.toml) |
| cargo test | the tests; the export tests decode their output with `ffmpeg` when it is on the `PATH` | `cargo test --workspace --features $F` | |
| [cargo-insta](https://insta.rs) | text snapshots | `cargo insta review` | `tests/snapshots/` of each crate |
| rustdoc | docs without warnings | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | |
| [cargo-hack](https://github.com/taiki-e/cargo-hack) | every feature combination compiles | `cargo hack check --workspace --feature-powerset --depth 2` | |
| [cargo-machete](https://github.com/bnjbvr/cargo-machete) | no unused dependency | `cargo machete` | |
| MSRV | builds at the workspace's `rust-version` | `cargo +1.89 check --workspace --all-targets` | `[workspace.package]` of [`Cargo.toml`](Cargo.toml) |
| [typos](https://github.com/crate-ci/typos) | spelling | `typos` | [`_typos.toml`](_typos.toml) |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | advisories, licences, sources | `cargo deny check` | [`deny.toml`](deny.toml) |
| [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | optional, local only: code no test reaches | `cargo llvm-cov --workspace --features $F --summary-only` | |

The versions CI uses are pinned in the workflows. Beyond the checks:

- Timings are `#[ignore]` tests, run with
  `cargo test --release -p <crate> --test <file> -- --ignored --nocapture <name>`; their results go into
  [`docs/measurements.md`](docs/measurements.md) with the command.
- [`tools/compare-csharp`](tools/compare-csharp/) dumps WaveDude Next's results for `wavedude-cli compare` (.NET 10 SDK,
  `WAVEDUDE_NEXT_DIR`); its [`README.md`](tools/compare-csharp/README.md) has the commands.
- Releases: [git-cliff](https://git-cliff.org) ([`cliff.toml`](cliff.toml)) writes the changelog and the next version from the
  commits (`git cliff --unreleased` previews it); cargo-packager, cargo-auditable and cargo-cyclonedx build the
  packages and SBOMs in [`.github/workflows/package.yml`](.github/workflows/package.yml). [`docs/release.md`](docs/release.md) covers the flow, the keys,
  the icons and the file types.
- Dependabot ([`.github/dependabot.yml`](.github/dependabot.yml)) opens grouped updates, daily for crates (`chore(deps)`), weekly for
  actions (`ci(deps)`); [`.github/workflows/dependabot-automerge.yml`](.github/workflows/dependabot-automerge.yml) merges them when CI passes, except a major
  version.

## Commit messages

[Conventional commits](https://www.conventionalcommits.org/); git-cliff builds [`CHANGELOG.md`](CHANGELOG.md) and the next
version from them ([`cliff.toml`](cliff.toml)):

- `feat` is listed under Added, `fix` under Fixed, `perf` under Performance, `refactor` under Changed, `revert`
  under Reverted; `fix(deps)` and `chore(deps)` under Dependencies. `ci`, `docs`, `test`, `style`, `build` and
  `chore` stay out of the changelog.
- A breaking change (a file format, a CLI flag, a library API) carries `!` after the type or a
  `BREAKING CHANGE:` footer. Before 1.0 a feature bumps the patch and a breaking change the minor.
- The scope is optional: a crate's short name (`core`, `export`, `widgets`, `state`, `app`, `cli`) or an area
  (`ui`, `io`, `gpu`); `deps` is reserved for dependency bumps.
- The subject is one or two sentences in the present tense saying what the change does for the reader of the
  changelog, not how; no issue numbers in it (they go in the body).

## Use of AI

Assistants are fine; you are the author. Understand every line you submit, run the checks yourself, and say in
the pull request that an assistant was involved. A pull request whose author cannot answer questions about it is
closed.

## Licensing

Contributions are licensed GPL-3.0-or-later. WaveDude Next and Wave Dude 2D are read to learn what they do, never
how: no code from either is accepted (decision 0001).
