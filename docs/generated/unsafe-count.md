# Unsafe Block Count: 0

## Current Status

`ryg-rans-core`: `#![forbid(unsafe_code)]` - zero unsafe blocks.

`ryg-rans-simd`: Not yet implementated. Will contain architecture-specific
`unsafe` blocks for SSE4.1 intrinsics when added.

## Unsafe Block Policy

Every `unsafe` block in the SIMD crate must document:

1. **Preconditions**: What must be true before calling this code.
2. **Alignment assumptions**: Required alignment of input/output pointers.
3. **Bounds assumptions**: Minimum length or capacity requirements.
4. **CPU feature assumptions**: Required ISA extensions (SSE4.1, etc.).
5. **Soundness justification**: Why the operation is safe despite `unsafe`.

## Audit Trail

| Location | Lines | Purpose | Audit Date | Status |
|----------|-------|---------|------------|--------|
| — | — | — | — | No unsafe blocks |
