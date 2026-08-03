# Public rANS Workload v1

A deterministic, versioned **rANS workload derivation** from public
compression corpora.  The corpus bytes are *not* committed to this
repository; what is committed is the pinned source identity, the rights
record, the derivation policy, and the tooling that fetches and derives.

A raw corpus is **not** a rANS workload.  A workload is:

```text
public source bytes
+ source identity and hash
+ deterministic slicing
+ block-size schedule
+ model-construction policy
+ model-reuse schedule
+ codec/backend policy
+ thread schedule
+ cache policy
= reproducible rANS workload
```

## Source tiers

| Tier | Sources | Purpose |
|------|---------|---------|
| Canterbury | `canterbury-standard` (11 files), `canterbury-artificial` (4 pathological files), `canterbury-large` (E.coli, bible.txt, world192.txt) | heterogeneous small/medium files, pathological repetition, incompressible-like data |
| Large Text Compression Benchmark | `enwik8` (100 MB), `enwik9` (1 GB) — the first 10^8 / 10^9 bytes of a fixed Wikipedia XML dump | standardized sustained text/XML workloads; cross-validated against the publisher's published MD5/SHA-1 |
| Pizza & Chili | `pc-dna` (50/100/200 MB), `pc-english` (50/200/1024 MB), `pc-xml-50mb` (DBLP), `pc-proteins-50mb`, `pc-sources-50mb`, `pc-pitches-50mb` | domain/alphabet/compressibility diversity, increasing-size experiments |

See `sources.toml` for the official pages, owners, license statements, and
redistribution status of every source; see `LICENSE-NOTICES.md` for the
rights record.  Where redistribution is uncertain, the fetch tool
downloads only through `cargo xtask workload fetch` and nothing is
redistributed.

## Pinned identity

* `expected-source-hashes.json` — SHA-256 + byte length of every archive
  and every extracted file, measured at retrieval (2026-08-02) and
  cross-validated for enwik8/enwik9 against the publisher's checksums.
* `derivation.toml` — the deterministic derivation policy (block-size
  matrix, boundary sizes, model construction/reuse, codec, threads, cache
  policy, named schedules).  The canonical bytes are hashed into every
  manifest.
* Workload identity = source hashes + derivation policy hash.

## Commands

```sh
# Fetch + verify + safely extract all pinned sources into the cache
# directory (default ~/.cache/ryg-rans-rs/workloads/).  Hash/size
# mismatches abort; archives already present and matching are reused
# (offline support).  Never executes downloaded content.
cargo xtask workload fetch public-rans-v1

# Derive the schedules (smoke / 1g / mixed-16g / stress-64g) as a small
# slice manifest — tens of gigabytes are never materialized.
cargo xtask workload derive public-rans-v1

# --- Cache-behaviour classes on synthetic payloads (honestly labeled) ---
# Deterministic xorshift patterns with constant seeds — NOT corpus bytes.
# These force exact cache access patterns (one model, 65 models against 64
# slots, ...) that public data does not naturally produce.  The output is
# labeled synthetic-cache-stress-v1 / synthetic-cache-soak-v1.
cargo xtask workload synthetic-cache-stress public-rans-v1   # alias: stress
cargo xtask workload synthetic-cache-soak public-rans-v1     # alias: soak

# --- Genuine public-corpus execution (the derived schedule IS the input) ---
# Every block resolves source_id + source_sha256 + offset + length to the
# hash-verified extracted bytes, encodes with the block's declared
# codec/scale, decodes, and asserts byte-exact output.  --schedule selects
# the executed schedule (smoke / 1g / mixed-16g / stress-64g); schedules
# stream in bounded windows so the 16/64 GiB logical schedules never
# materialize.  Natural mode derives each model from the block's bytes;
# grouped mode trains one model per group from the declared public
# training region (fallbacks counted).
cargo xtask workload stress-public public-rans-v1 --schedule public-rans-smoke
cargo xtask workload soak-public public-rans-v1 --schedule public-rans-1g

# Offline eviction-policy shadow simulation (FIFO vs LRU).
cargo xtask workload policy-sim public-rans-v1
```

The fetch cache lives **outside the Git repository**; `RYG_RANS_WORKLOAD_DIR`
overrides the default location.

> **Workload identity honesty (post-v0.5.0 audit, `MODEL_CACHE.WORKLOAD.2`):**
> requiring the fetched corpus to *exist* is not the same as deriving the
> executed workload from its *bytes*.  Only `stress-public` / `soak-public`
> (and the Criterion `model_cache/public` group) may claim public-corpus
> provenance; the synthetic runners never do.

## Derived schedules

| Schedule | Logical bytes | Purpose |
|----------|---------------|---------|
| `public-rans-smoke` | 4 MiB | every source family, every block size + boundary triples, grouped + natural models |
| `public-rans-1g` | 1 GiB | sustained text/XML + DNA with 64 KiB–4 MiB blocks |
| `public-rans-mixed-16g` | 16 GiB | 16 passes over a deterministic 1 GiB slice composition (source-region reuse, never materialized) |
| `public-rans-stress-64g` | 64 GiB | 4 rotations of the mixed composition with offset/group variation — worst-case cache churn + phase shifts |

Every block record is `(source_id, source_sha256, offset, length,
model_group, codec_id, scale_bits)`; the schedule hash covers the ordered
records.

## Model reuse — natural vs grouped (Phase O.13)

Natural per-block histograms rarely produce byte-identical model
encodings, so the workload distinguishes two modes explicitly and **never
presents one as the other**:

* **Natural model mode** (`model_group == u64::MAX` in the manifest):
  each block derives its own model from its own bytes.  The cache reuse
  is whatever occurs organically — measured and reported as the actual
  model cardinality and hit rate, never engineered upward.

* **Grouped model mode** (any other `model_group`): a model is derived
  from a declared training region and reused for a deterministic group of
  blocks.  The training region is public-corpus-derived (per
  `derivation.toml`: `source[g % num_sources]`, bytes `[0,
  training_region_bytes)`); the group assignment is deterministic; blocks
  of one group are encoded with the group's model via
  `ModelPolicy::External` (implemented in Phase O.13).  Results produced
  in this mode are labeled `grouped-model` in every measurement that uses
  them.

Both modes are required by the workload: natural measures organic reuse;
grouped exercises the intended application behavior (an application that
reuses one model across many blocks).  Synthetic cache-friendly workloads
are never presented as naturally-occurring behavior — the `unique`
workload class exists precisely to prove cache overhead when no reuse
exists, and the `public-rans-smoke` schedule mixes both modes so the
honest organic rate is measurable.

## Cache policy classes

The derivation declares the cache budgets each schedule assumes
(`[cache]`): `disabled` (zero capacity — the semantic baseline), `default`
(64 entries / 16 MiB), and `hot_set`.  The benchmark harness (Phase
O.14) additionally defines the disabled / cold / warm / hot-set / thrash /
unique measurement classes; see `docs/performance/model-cache.md` for
what each class proves and what it deliberately does not.
