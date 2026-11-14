# 0026: The run worker kicks the GPU queue while it waits

Status: accepted, 2026-11-14. Amends [0024](0024-browser-run-in-a-web-worker.md): the worker's wait on the GPU.

## Context

The run worker waits on the GPU's callbacks: `on_submitted_work_done` paces its submissions, `mapAsync` lands its
capture and probe readbacks. Firefox resolves both only when its GPU process polls the device: on a submit, or from a
100 ms timer ([Mozilla bug 1870699](https://bugzilla.mozilla.org/show_bug.cgi?id=1870699), "Don't poll WebGPU from a
timer", open). Since captures and probe readbacks ride their steps' batch, the run submits about once per 32 steps
and then waits without submitting, so each completion lands up to 100 ms late: Example01 ran 4 s in Firefox against
1.1 s in Chromium, the worker mostly waiting on capture readbacks. A page measured the same on a single readback
([`docs/measurements.md`](../measurements.md), The browser demo): 100 ms in Firefox, 8 ms with empty submits.

## Decision

- The `GpuContext` counts the completions its engines await: a submission until the card reports it run, a mapping
  from its request until it lands (`awaits_completions`); `kick` submits an empty command list.
- While the run worker is parked (`Pump::Waiting`, whatever it waits for) and its device, not lost, owes
  completions, it kicks the queue on `Kicks`: the first after 0.5 ms, the gap doubling up to 8 ms, until a wake ends
  the wait or nothing is owed. The timeouts count whole milliseconds, so the gaps are rounded up. It never kicks
  while nothing is owed, on a lost device, or idle. The schedule follows go-gfx/webgpu #24 and PlayCanvas
  supersplat-viewer #359.

## Why uniform

No user-agent check: a browser that resolves completions on its own does so before the first kick or ignores it, and
Firefox's behaviour can change without its user agent saying so.

## Consequences

- In Firefox a completion lands at most 8 ms after the card ran it, not 100 ms.
- An empty submit and a timeout per gap while the worker waits on the GPU; none while idle. Natively the counting
  costs a callback per submission and nothing kicks.

## Removal

When the browsers resolve `onSubmittedWorkDone` and `mapAsync` without polling from a timer (bug 1870699 fixed in
the Firefox releases supported), the kicks and the count go.
