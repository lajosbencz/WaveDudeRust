# Documentation

- [`spec/`](spec/): what the core and export crates must do, mapped from WaveDude Next (step 1 of the plan).
- [`ui/`](ui/): what the desktop UI must do, mapped from WaveDude Next's WPF app (step 4).
- [`decisions/`](decisions/): architecture decision records, one file per decision, numbered.
- [`blockers.md`](blockers.md): upstream bugs worked around, each with its workaround and where to report it.
- [`divergences.md`](divergences.md): what differs from WaveDude Next on purpose, one line each, pointing to the decisions.
- [`conventions.md`](conventions.md): how the code is written, each rule with a place in the code that shows it.
- The crates are listed in [`WORKSPACE.md`](../WORKSPACE.md) ([decision 0032](decisions/0032-slint-single-frontend.md)).
- The frontend is rebuilt on Slint ([decision 0032](decisions/0032-slint-single-frontend.md)); [`rebuild.md`](rebuild.md) maps the app it replaces. [`parity.md`](parity.md) lists every feature of WaveDude Next and its state in the Slint app. What of Next's UI is matched: [decision 0033](decisions/0033-ui-match-scope.md).
- [`physics.md`](physics.md): discretisation, stability, dispersion and absorbers per the literature the engine tests follow.
- [`figures/`](figures/): pictures the README shows.
