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
```

The fetch cache lives **outside the Git repository**; `RYG_RANS_WORKLOAD_DIR`
overrides the default location.

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
