# 0001 From-scratch Rust reimplementation of WaveDude Next

Status: accepted, 2026-11-05

WaveDude Next (C#, .NET 10 + WPF) is Windows-only. Its code is almost entirely
LLM-generated (every commit co-authored by Claude, 166 commits in four weeks).
The user decisions and measurements in its notes are trusted; its code is not
carried over.

Decision: a new repository, lajosbencz/WaveDudeRust, one Cargo workspace,
GPL-3.0-or-later (the original's license; the spec is derived from reading its
source, so the result is a derivative work in spirit). Credits to Alexander Graef
(WaveDude Next) and Jeroen Vleggaar (Wave Dude 2D) in README and About.
Not a GitHub fork: no code is shared, so the fork relation would be noise.

Plan: 1 map core features -> 2 core crate with tests -> 3 compare with C# ->
4 map UI -> 5 widget crate -> 6 Freya app. Steps 1 and 4 run in parallel.
The C# source is read to discover features and later to compare details; the
implementation is written from the spec.
