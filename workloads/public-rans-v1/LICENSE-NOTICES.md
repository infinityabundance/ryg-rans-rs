# License notices — public-rans-v1 sources

This repository does **not** redistribute third-party corpus bytes.  The
fetch tool downloads from the official endpoints below into a cache
directory outside the repository.  This file records the rights status of
every source, honestly including where it is uncertain.

Retrieval date for all records: 2026-08-02.

## Canterbury family — https://corpus.canterbury.ac.nz

| Source | Owner | License / usage | Redistribution |
|--------|-------|-----------------|----------------|
| `canterbury-standard` | Canterbury Corpus project (R. Arnold, T. Bell, University of Canterbury) | free for research and evaluation | permitted (public corpus; no restriction stated) |
| `canterbury-artificial` | same | free for research and evaluation | permitted |
| `canterbury-large` | same; underlying texts public domain (bible translations, CIA World Factbook, GenBank E.coli) | free for research and evaluation | permitted |

## Large Text Compression Benchmark — https://mattmahoney.net/dc

| Source | Owner | License / usage | Redistribution |
|--------|-------|-----------------|----------------|
| `enwik8` | Matt Mahoney (benchmark data; fixed snapshot of Wikipedia content) | public-domain benchmark data | permitted (the published zip is the benchmark artifact); extracted bytes verified byte-identical to the publisher's checksums |
| `enwik9` | same | public-domain benchmark data | permitted |

Wikipedia content is CC BY-SA; the benchmark zips are the canonical
fixed-snapshot artifacts published for the benchmark and are the standard
object of study for the Large Text Compression Benchmark.

## Pizza & Chili — https://pizzachili.dcc.uchile.cl

| Source | Owner | License / usage | Redistribution |
|--------|-------|-----------------|----------------|
| `pc-dna-*` | Pizza&Chili Corpus project; underlying data from GenBank | free for research and academic use (site terms) | **uncertain** — download only via the fetch tool; do not redistribute |
| `pc-english-*` | P&C project; text from Project Gutenberg | free for research and academic use | **uncertain** — do not redistribute |
| `pc-xml-50mb` | P&C project; data from DBLP (M. Ley) | free for research and academic use | **uncertain** — do not redistribute |
| `pc-proteins-50mb` | P&C project; data from UniProt/SwissProt | free for research and academic use | **uncertain** — do not redistribute |
| `pc-sources-50mb` | P&C project; data from kernel.org / gnu.org | free for research and academic use | **uncertain** — do not redistribute |
| `pc-pitches-50mb` | P&C project | free for research and academic use | **uncertain** — do not redistribute |

The Pizza & Chili downloads are used for research evaluation only and are
not redistributed in any form by this repository or its release artifacts.

## Policy

1. Never commit corpus bytes to the repository.
2. Never redistribute a source whose redistribution status is uncertain.
3. Record the official page, owner, license statement, retrieval date, and
   expected hashes for every source (`sources.toml`,
   `expected-source-hashes.json`).
4. If a source's terms change, update this record and the manifest before
   the next fetch.
