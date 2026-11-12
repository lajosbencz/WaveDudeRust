# Agents

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/SEED.md`](docs/SEED.md) first; [`README.md`](README.md) and [`docs/README.md`](docs/README.md) map the code and docs.

- Git: changes reach master through pull requests ([`CONTRIBUTING.md`](CONTRIBUTING.md)); commit, push or tag only when asked.
- Temporary files go outside the repository.
- [`tools/compare-csharp`](tools/compare-csharp/) dumps WaveDude Next's results for comparison with ours; it finds Next through `WAVEDUDE_NEXT_DIR`.
  The comparison verifies, it never calibrates ([`docs/divergences.md`](docs/divergences.md)).
- WaveDude Next is ported by hand when asked: diff `graealex/WaveDudeNext` from the last reviewed commit, `a8ffb90`, triage
  each commit, port features, not code, one commit per feature, then update that commit here.
- Input is never injected on the desktop in use: UI comparisons run on a nested display
  ([`tools/compare-csharp/README.md`](tools/compare-csharp/README.md), Next's UI on Linux).
- Planned multi-phase work: one coding agent per phase, another agent verifies the diff and the checks before the commit.
- Verify with the checks of [`CONTRIBUTING.md`](CONTRIBUTING.md), Tools.
