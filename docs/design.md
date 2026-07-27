# Design status

The accepted input is Draft v0.4, dated 2026-07-27. Its key instruction is to
stop refining the provisional API and answer Phase 0 questions Q1–Q7 on a real
Windows + AviUtl2 host first.

This repository therefore implements only the Phase 0 harness:

- Rust workspace split into `protocol`, `plugin`, and `cli`;
- Linux Docker as the canonical Windows MSVC cross-build;
- an AviUtl2 generic plugin with a loopback `/healthz` server;
- clean server shutdown through plugin destruction;
- a CLI health probe;
- the reproducible test/result ledger in [`phase0.md`](phase0.md).

The stable architectural constraints retained from v0.4 are:

- SDK types never cross the future HTTP contract;
- all SDK calls will pass through one timeout-aware editor gate;
- HTTP workers must never acquire the plugin singleton lock; SDK state needed
  by workers must be held independently so unload cannot deadlock while
  dropping the singleton and joining workers;
- health/status paths must remain independent of that gate;
- writes follow `inspect → validate → apply → verify`;
- writes identify project epoch, scene, revision, and target explicitly;
- the plugin never saves the AviUtl2 project;
- unconditional agent-accessible Undo/Redo is forbidden;
- Linux cross-build and Windows runtime verification remain separate gates.

The complete v0.4 source remains the project input. After Phase 0, replace
unverified branches with observed facts and produce a shorter v0.5 here before
starting the read-only API.
