# Architecture Atlas (N.5)

> Chaptered deep dives into each major architecture, each with diagrams.
> The atlas is the "zoom in" layer: papers explain *why*, the atlas
> explains *how the parts fit*.

## Chapters

| Chapter | File | Covers |
|---------|------|--------|
| 1. Repository overview | `atlas-01-repository.md` | crates, dependency graph, doc layers |
| 2. Encoding architecture | `atlas-02-encoding.md` | block selection, codec dispatch, container write |
| 3. Decoding architecture | `atlas-03-decoding.md` | container read, plan, execute, verify |
| 4. Model lifecycle | `atlas-04-model-lifecycle.md` | frequencies → artifacts → cache → tables |
| 5. Evidence lifecycle | `atlas-05-evidence-lifecycle.md` | court → manifest → receipt → index → seal |
| 6. Performance lifecycle | `atlas-06-performance-lifecycle.md` | benchmark → preflight → export → seal |
| 7. Release lifecycle | `atlas-07-release-lifecycle.md` | freeze → regenerate → publish → tag |
| 8. Parallel scheduler | `atlas-08-parallel-scheduler.md` | producer, channels, workers, coordinator |
| 9. SIMD hierarchy | `atlas-09-simd-hierarchy.md` | scalar → SSE4.1 → AVX2 → AVX-512 |
| 10. Oracle architecture | `atlas-10-oracle.md` | C adapter, courts, promote-merge |
| 11. CLI architecture | `atlas-11-cli.md` | subcommands, dispatcher, integrity, exit codes |
