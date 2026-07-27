# aviutl2-agent

A Phase 0 spike for controlling a running AviUtl2 project through a local,
structured API. The product architecture is intentionally not implemented yet:
the first milestone is to measure the AviUtl2 Plugin SDK behavior documented in
[`docs/phase0.md`](docs/phase0.md).

## Developer checks

```bash
cargo test --workspace
cargo run -p aviutl2-agent -- health
```

The health command expects the Windows plugin probe to listen at
`http://127.0.0.1:7890`.

## Windows Phase 0 smoke test

1. Copy `dist/aviutl2-agent-plugin.aux2` into AviUtl2's plugin directory.
2. Start AviUtl2 and confirm `aviutl2-agent Phase 0` appears in plugin info.
3. Run `dist\aviutl2-agent.exe health`.
4. Close AviUtl2, start it again, and repeat step 3. A successful restart
   confirms that plugin teardown released the listening socket.

Port 7890 is intentionally fixed only for the first single-instance spike.
Session discovery and collision-free dynamic ports belong to Phase 1.
If another process already owns port 7890, `InitializePlugin` returns failure
and AviUtl2 will not load this Phase 0 plugin instance.

## Cross build

```bash
docker build --output type=local,dest=dist .
```

This should produce `aviutl2-agent-plugin.aux2`, `aviutl2-agent.exe`, and
`SHA256SUMS`. Loading the plugin and all SDK-dependent checks require a Windows
machine with AviUtl2.
