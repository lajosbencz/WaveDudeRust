# Single source of the checks: .github/workflows/{ci,hygiene}.yml call these recipes.
# Tools: `just tools`; cargo-nextest, cargo-hack, cargo-shear, cargo-deny, typos, ast-grep, zizmor, cargo-public-api,
# a nightly toolchain (rustdoc JSON), dx; actionlint and shellcheck from their releases or the system.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Nightly for rustdoc JSON (api-diff); at least a week old.
nightly_version := "nightly-2026-11-12"

# Tool pins, read by the workflows (`just --evaluate <name>`).
nextest_version := "0.9.152"
hack_version := "0.6.46"
shear_version := "1.13.4"
deny_version := "0.20.2"
typos_version := "1.52.2"
ast_grep_version := "0.51.2"
# Release asset digest (x86_64 Linux), CI only.
ast_grep_sha256 := "40604c21c38d4c7581c56bdddeef277c56bf3c0113620aa96e78ae87e82366df"
zizmor_version := "1.32.0"
public_api_version := "0.52.0"

# OpenH264 compiled in, for the tests and clippy (never for distributed builds).
features := "waveduderust/openh264-source,waveduderust_cli/openh264-source"

# Crates built for the browser.
wasm_crates := "waveduderust waveduderust_platform waveduderust_slint"

# Crates without a UI toolkit, and the toolkits they must not reach.
headless_crates := "waveduderust waveduderust_macros waveduderust_platform waveduderust_cli"
toolkits := "slint egui eframe gtk4 libadwaita cxx-qt"

# The library crates whose public API the diff guards.
api_crates := "waveduderust"

# List the recipes
default:
    @just --list

# Installs the check tools at the pinned versions, and the nightly toolchain
tools:
    cargo install --locked cargo-nextest@{{ nextest_version }} cargo-hack@{{ hack_version }} cargo-shear@{{ shear_version }} cargo-deny@{{ deny_version }} typos-cli@{{ typos_version }}
    cargo install --locked ast-grep@{{ ast_grep_version }} zizmor@{{ zizmor_version }} cargo-public-api@{{ public_api_version }}
    rustup toolchain install {{ nightly_version }} --profile minimal
    rustup target add wasm32-unknown-unknown
    @command -v actionlint >/dev/null || echo "actionlint is not on crates.io: https://github.com/rhysd/actionlint"
    @command -v shellcheck >/dev/null || echo "shellcheck is not on crates.io: install it with the system package manager"

# Every blocking check of CI and Hygiene, but the web build and the platforms
ci: check hygiene

# Fast local loop: format, workspace clippy, tests
quick: fmt clippy-native test doctest

# Formats and applies machine-applicable clippy fixes
fix:
    cargo fmt --all
    cargo clippy --fix --allow-dirty --workspace --all-targets --features {{ features }}

# The CI job "ubuntu-latest" without the platforms
check: fmt clippy test doctest doc features shear layers versions-check

fmt:
    cargo fmt --all --check

# Native and wasm32
clippy: clippy-native clippy-wasm

clippy-native:
    cargo clippy --workspace --all-targets --features {{ features }} -- -D warnings

# Each feature alone, the default set and none, per browser crate; std's clocks banned (tools/clippy-wasm32)
clippy-wasm:
    #!/usr/bin/env bash
    set -euo pipefail
    export CLIPPY_CONF_DIR=tools/clippy-wasm32
    for c in {{ wasm_crates }}; do
        case $c in
        # Downloaded or compiled OpenH264 and its CLI flags are native only.
        waveduderust) ex=(--exclude-features default,mp4,openh264-source,clap) ;;
        *) ex=() ;;
        esac
        cargo hack clippy -p "$c" --each-feature "${ex[@]}" --target wasm32-unknown-unknown -- -D warnings
    done
    # The browser's set of the library, which each-feature leaves out
    cargo clippy -p waveduderust --no-default-features --features gpu,run,serde,mp4-mux --target wasm32-unknown-unknown -- -D warnings

# Tests (nextest, no doctests); extra args go to nextest, e.g. `--no-run`
test *args:
    cargo nextest run --workspace --features {{ features }} {{ args }}

doctest:
    cargo test --doc --workspace --features {{ features }}

# The library alone too: in the workspace its consumers turn the unstable features on
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    RUSTDOCFLAGS="-D warnings" cargo doc -p waveduderust --no-deps

# Every feature pair of every crate compiles
features:
    cargo hack check --workspace --feature-powerset --depth 2

# No unused dependency
shear:
    cargo shear

# No headless crate depends on a UI toolkit
layers:
    #!/usr/bin/env bash
    set -euo pipefail
    bad=0
    for c in {{ headless_crates }}; do
        deps=$(cargo tree -p "$c" -e normal --prefix none --format '{p}' | awk '{print $1}' | sort -u)
        for t in {{ toolkits }}; do
            if grep -qx "$t" <<<"$deps"; then echo "$c depends on $t"; bad=1; fi
        done
    done
    exit $bad

# Crates share MAJOR.MINOR with the app (0.MINOR before 1.0); the pinned toolchain is the workspace MSRV
versions-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo metadata --no-deps --format-version 1 | jq -e '
        [.packages[] | .version | split(".") | if .[0] == "0" then .[0:2] else .[0:1] end | join(".")]
        | unique | length == 1' >/dev/null \
        || { echo "crates differ in MAJOR.MINOR"; exit 1; }
    channel=$(sed -nE 's/^channel *= *"([0-9]+\.[0-9]+)(\.[0-9]+)?"/\1/p' rust-toolchain.toml)
    msrv=$(sed -nE '/^\[workspace\.package\]/,/^\[/s/^rust-version *= *"([0-9]+\.[0-9]+)(\.[0-9]+)?"/\1/p' Cargo.toml)
    [ -n "$channel" ] && [ "$channel" = "$msrv" ] \
        || { echo "rust-toolchain.toml channel (${channel:-unset}) must match workspace rust-version (${msrv:-unset})"; exit 1; }

# Public API of the library crates against <base> (a git ref); `unstable-*` features are left off
api-diff base="origin/master":
    #!/usr/bin/env bash
    set -euo pipefail
    for c in {{ api_crates }}; do
        cargo +{{ nightly_version }} public-api -p "$c" -sss diff "{{ base }}..HEAD"
    done

# Spelling, dependencies, workflows, ast-grep rules
hygiene: typos deny workflows lint

typos:
    typos

deny:
    cargo deny check

workflows:
    actionlint
    zizmor .github

# ast-grep rules (lints/rules) and their tests (lints/tests)
lint:
    ast-grep test --skip-snapshot-tests
    ast-grep scan

# The browser app (tools/build-web.sh): wasm-bindgen of Cargo.lock's version and wasm-opt on the PATH
web out="":
    tools/build-web.sh {{ out }}

# The app with its `.slint` files reloaded on save (Slint live preview); Rust changes need a rerun
hot:
    SLINT_LIVE_PREVIEW=1 cargo run -p waveduderust_slint --features slint/live-preview
