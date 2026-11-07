# Reference ports

Python ports of WaveDude Next's logic gate design (`gate.py`) and slab
waveguide mode solver (`modes.py`), written while deriving `docs/spec/21` and
`22` and checked against the numbers `OpticalParts.md` documents. They are the
oracle for the Rust `scene::gate_design` and `scene::modes` tests, not part of
any build. Run with `python3 -I gate.py`.
