# Contributing

Read first: the ground rules in [`docs/SEED.md`](docs/SEED.md), the conventions the code follows in [`docs/conventions.md`](docs/conventions.md), and
what differs from WaveDude Next and why in [`docs/divergences.md`](docs/divergences.md). [`docs/README.md`](docs/README.md) maps the documentation.

## Issues

Search the open and closed issues first. The forms under [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE/) ask for what each kind needs:

- **Bug**: the version (Help > About, or `wavedude-cli --version`), the platform, the steps, what was expected,
  the project file or the name of the example used, and for a wrong picture or export the output itself.
- **Crash**: the same plus the log of the day, which holds the panic's backtrace; a panic that ended the program
  also left `crash.txt` beside it, which the next start shows once and renames `last-crash.txt`. The logs folder is
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

The compiler is pinned in [`rust-toolchain.toml`](rust-toolchain.toml) with clippy and rustfmt; `rustup toolchain install` in
the repository installs it, as CI does. Bump it there when a new stable is wanted (the CI caches are keyed on it). Linux needs the
packages of the "Linux packages for Skia and winit" step of [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Below,
`F=wavedude-export/openh264-source,wavedude-cli/openh264-source`.

| Tool | Checks | Run | Configured in |
|---|---|---|---|
| rustfmt | formatting | `cargo fmt --all` | [`rustfmt.toml`](rustfmt.toml) |
| clippy | lints, warnings fail | `cargo clippy --workspace --all-targets --features $F -- -D warnings` | `[workspace.lints]` of [`Cargo.toml`](Cargo.toml) |
| cargo test | the tests; the export tests decode their output with `ffmpeg` when it is on the `PATH` | `cargo test --workspace --features $F` | |
| [cargo-insta](https://insta.rs) | text snapshots | `cargo insta review` | `tests/snapshots/` of core and export |
| rustdoc | docs without warnings | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | |
| [cargo-hack](https://github.com/taiki-e/cargo-hack) | every feature combination compiles | `cargo hack check --workspace --feature-powerset --depth 2` | |
| [cargo-machete](https://github.com/bnjbvr/cargo-machete) | no unused dependency | `cargo machete` | |
| MSRV | builds at the workspace's `rust-version` | `rustup toolchain install 1.89`, then `cargo +1.89 check --workspace --all-targets` | `[workspace.package]` of [`Cargo.toml`](Cargo.toml) |
| wasm32 | the core without `run`, with and without `gpu`, and the view crate build for the browser and are clippy-clean there | `rustup target add wasm32-unknown-unknown`, then `cargo clippy -p wavedude-core --no-default-features --target wasm32-unknown-unknown -- -D warnings`, the same with `--features gpu`, and `cargo clippy -p wavedude-view --target wasm32-unknown-unknown -- -D warnings` | `[features]` of [`crates/wavedude-core/Cargo.toml`](crates/wavedude-core/Cargo.toml) |
| wasm package | the wasm package is clippy-clean for the browser | `cargo clippy -p wavedude-wasm --target wasm32-unknown-unknown -- -D warnings` | [`crates/wavedude-wasm/Cargo.toml`](crates/wavedude-wasm/Cargo.toml) |
| [typos](https://github.com/crate-ci/typos) | spelling | `typos` | [`_typos.toml`](_typos.toml) |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | advisories, licences, wildcard versions, sources | `cargo deny check` | [`deny.toml`](deny.toml) |
| [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | optional, local only: code no test reaches | `cargo llvm-cov --workspace --features $F --summary-only` | |

The versions CI uses are pinned in the workflows. Coverage is slow: measured on 24 threads, its tests took 43 minutes
after a build of its own; instrumented, `cli.rs` went from 21 s to 23.5 min and `physics.rs` from 14 s to 17 min. Beyond
the checks:

- Timings: `wavedude-cli bench` (`--help` lists its options) and the `#[ignore]` timing tests, run with
  `cargo test --release -p <crate> --test <file> -- --ignored --nocapture <name>`; a result goes into
  [`docs/measurements.md`](docs/measurements.md) with its command.
- [`tools/compare-csharp`](tools/compare-csharp/) dumps WaveDude Next's results for `wavedude-cli compare` (.NET 10 SDK,
  `WAVEDUDE_NEXT_DIR`); its [`README.md`](tools/compare-csharp/README.md) has the commands.
- The web demo ([`demo/`](demo/), decision 0017): `cargo install --locked wasm-bindgen-cli --version <the wasm-bindgen version in Cargo.lock>`,
  `rustup target add wasm32-unknown-unknown`, then `demo/build.sh target/site` and
  `python3 -m http.server -d target/site`; open http://127.0.0.1:8000 (WebGPU needs a secure context, and localhost is one).
- Releases: [git-cliff](https://git-cliff.org) ([`cliff.toml`](cliff.toml)) writes the changelog and the next version from the
  commits (`git cliff --unreleased` previews it); cargo-packager, cargo-auditable and cargo-cyclonedx build the
  packages and SBOMs in [`.github/workflows/package.yml`](.github/workflows/package.yml). [`docs/release.md`](docs/release.md) covers the flow, the keys,
  the icons and the file types.
- Dependabot ([`.github/dependabot.yml`](.github/dependabot.yml)) opens grouped updates, daily for crates (`chore(deps)`), weekly for
  actions (`ci(deps)`), each version once it is a week old (security updates at once); [`.github/workflows/dependabot-automerge.yml`](.github/workflows/dependabot-automerge.yml) enables auto-merge on them, except a major
  version, so they merge once the required checks pass (the ruleset of [`docs/release.md`](docs/release.md)).

## Commit messages

[Conventional commits](https://www.conventionalcommits.org/); git-cliff builds [`CHANGELOG.md`](CHANGELOG.md) and the next
version from them ([`cliff.toml`](cliff.toml)):

- `feat` is listed under Added, `fix` under Fixed, `perf` under Performance, `refactor` under Changed, `revert`
  under Reverted; `fix(deps)` and `chore(deps)` under Dependencies. `ci`, `docs`, `test`, `style`, `build` and
  `chore` stay out of the changelog.
- A breaking change (a file format, a CLI flag, a library API) carries `!` after the type or a
  `BREAKING CHANGE:` footer. Before 1.0 a feature bumps the patch and a breaking change the minor.
- The scope is optional: a crate's short name (`core`, `view`, `export`, `widgets`, `state`, `app`, `cli`, `examples`, `wasm`) or an area
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
