# 00 — First Day

**Purpose:** Orient a newcomer with no prior rANS or Rust experience in under
an hour: what this repository is, what it is not, and where to look first.

**Prerequisites:** none.

**Required papers:** `docs/papers/0001-rans-design.md` §1–§5 (skim).

**Required ADRs:** none.

**Required source modules:** none — read prose first.

**Recommended reading order:**
1. `README.md` — the portal; read the entry-point section matching you.
2. `docs/philosophy.md` — why the repository is written this way.
3. `docs/glossary.md` — the exact terms.
4. `docs/layers.md` — where each kind of fact lives.
5. `docs/papers/0001-rans-design.md` §1–§5 — the arithmetic in prose.
6. Try the CLI: `cargo run -- encode -i <file> -o out.rygr` then `decode`.

**Expected understanding:** what rANS is, what the repository's evidence
doctrine is, how to run the CLI, and which documents to read next.

**Estimated reading time:** 45–60 minutes.

**Exercises:**
1. Encode a file, decode it, verify `cmp` equality.
2. Run `cargo xtask seal` once and watch the gate list.
3. Find the glossary term "surface" and name the seven surfaces.

**Common misconceptions:**
- "This is just a library." It is also a reference, a corpus, a textbook,
  and a case study (see `README.md` identity section).
- "Comments are too long." They preserve invariants the code cannot
  express (philosophy §5).

**Related evidence:** the README evidence table; `evidence/index.json`.

**Future reading:** `docs/navigation/01-first-week.md`.
