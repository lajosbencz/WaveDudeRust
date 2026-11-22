# Documentation

- [`spec/`](spec/): what the core and export crates must do, mapped from WaveDude Next (step 1 of the plan).
- [`ui/`](ui/): what the desktop UI must do, mapped from WaveDude Next's WPF app (step 4).
- [`decisions/`](decisions/): architecture decision records, one file per decision, numbered.
- [`blockers.md`](blockers.md): upstream bugs worked around, each with its workaround and where to report it.
- [`divergences.md`](divergences.md): what differs from WaveDude Next on purpose, one line each, pointing to the decisions.
- [`conventions.md`](conventions.md): how the code is written, each rule with a place in the code that shows it.
- The crates are listed in [`WORKSPACE.md`](../WORKSPACE.md); the layout they move to is [decision 0031](decisions/0031-modular-core-and-frontends.md).
- [`physics.md`](physics.md): discretisation, stability, dispersion and absorbers per the literature the engine tests follow.
- [`figures/`](figures/): pictures the README shows.
