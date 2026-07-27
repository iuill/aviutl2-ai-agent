# Phase 0: SDK fact-finding

This is a results ledger, not an API specification. Record AviUtl2 version,
`aviutl2` crate version, build origin, exact reproduction steps, observed
result, and logs for every check. Do not advance to Phase 1 while any conclusion
below is still `UNTESTED`.

## Test environment

| Field | Value |
|---|---|
| AviUtl2 | UNTESTED |
| `aviutl2` crate | 0.41.0 |
| Rust | 1.88.0 |
| Cross-build image digest | UNTESTED |
| Windows version | UNTESTED |

## Bootstrap

- [x] Linux Docker produces `.aux2` and `.exe`.
- [ ] `.aux2` loads and registers.
- [ ] `GET /healthz` responds.
- [ ] CLI parses the health response.
- [ ] Plugin unload stops and joins every HTTP worker.
- [ ] Determine whether AviUtl2 unloads the DLL during the process lifetime or
      only as part of process exit.
- [ ] A long SDK operation does not block `/healthz`.

## Q1 — section threading and reentrancy

Status: **UNTESTED**

- [ ] Read section from an HTTP worker.
- [ ] Edit section from an HTTP worker.
- [ ] Concurrent read/read, read/write, and write/write.
- [ ] Call from an event callback.
- [ ] Read current state inside an edit section.
- [ ] Nested and consecutive edit sections.
- [ ] Shutdown while a section is active.

Record the allowed calling thread and the required dispatcher design:

> UNTESTED

## Q2 — Undo and partial failure

Status: **UNTESTED**

- [ ] Change two objects in one edit section; perform one Undo.
- [ ] Intentionally fail the second mutation.
- [ ] Create an object, then fail a setting update.
- [ ] Delete in a compound operation, then Undo.
- [ ] Locate any explicit rollback API.

Record Undo granularity and whether partial effects remain:

> UNTESTED

## Q3 — frame rendering

Status: **UNTESTED**

- [ ] Render an explicit scene/frame.
- [ ] Record caller and callback threads.
- [ ] Copy pixels into an owned buffer before returning.
- [ ] Record pixel format, pitch, alpha, and buffer lifetime.
- [ ] Determine whether render size is selectable.
- [ ] Render during playback, export, and a modal dialog.
- [ ] Measure repeated and large-resolution calls.
- [ ] Determine cancellation behavior.

> UNTESTED

## Q4 — editor busy states

Status: **UNTESTED**

Try read/write/render during timeline drag, modal dialog, playback, export,
project load/save, Undo/Redo, and shutdown. Record whether the state can be
detected before making an SDK call.

> UNTESTED

## Q5 — events, revisions, and handles

Status: **UNTESTED**

Log events and handles for create, update, move, delete, effect change,
scene switch, API-originated changes, Undo/Redo, and project reload. Record
whether events are synchronous, duplicated, or missing and whether deleted
handles are reused.

> UNTESTED

## Q6 — Linux-to-Windows build

Status: **IN PROGRESS**

- [x] `cargo xwin` builds plugin and CLI.
- [x] Rename DLL to `.aux2`.
- [x] No extra runtime DLL is needed.
- [ ] Cross-built and Windows-native artifacts both load.

Cross-build completed on 2026-07-27. The PE export table contains the expected
generic-plugin ABI (`RequiredVersion`, `InitializePlugin`, `RegisterPlugin`,
`UninitializePlugin`, and related initialization exports). Windows loading is
still untested and must not be inferred from this result. Both artifacts use
static MSVC CRT linking; PE import inspection shows only Windows system DLLs.

## Q8 — plugin unload and owned threads

Status: **IN PROGRESS**

The first `tiny_http` spike was rejected because its internal keep-alive task
could outlive the server value and continue executing DLL code after unload.
The Phase 0 server now owns every worker directly, closes every response
connection, and joins all workers in `Drop`; Linux regression tests cover an
idle keep-alive client and port rebinding.

- [ ] Determine whether AviUtl2 calls `UninitializePlugin` before `FreeLibrary`.
- [ ] Verify no plugin thread exists after `UninitializePlugin`.
- [ ] Verify an idle client cannot delay unload.
- [ ] Verify repeated load/unload or process restart releases the port.

> Windows runtime behavior remains UNTESTED.

## Q7 — Undo API exposure

Status: **UNTESTED**

- [ ] Locate an SDK Undo/Redo API.
- [ ] Determine whether it can undo a human operation.
- [ ] Record events and revision behavior.
- [ ] Determine whether stack position/depth is queryable.

Do not expose Undo unless the immediately preceding API operation can be proven
to be the only operation affected.

> UNTESTED

## Result

Phase 0 completion decision: **NOT READY**

After all results are recorded, replace unverified branches in the design with
observed facts and write a shorter v0.5 before implementing the read API.
