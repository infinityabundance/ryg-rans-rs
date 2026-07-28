//! # ryg-rans-rs-cli
//!
//! **Command-line tools for rANS entropy coding.**
//!
//! ## Planned Commands
//!
//! | Command | Status | Description |
//! |---------|--------|-------------|
//! | `encode` | ✗ | Encode input using rANS with a frequency model |
//! | `decode` | ✗ | Decode a rANS-compressed stream |
//! | `inspect` | ✗ | Inspect a compressed stream: state, size, model |
//! | `trace` | ✗ | Emit per-symbol state transition traces |
//! | `compare` | ✗ | Compare two implementations for parity |
//! | `bench` | ✗ | Measure encoding/decoding throughput |

fn main() {
    println!("ryg-rans-rs-cli - rANS encoding tools");
    println!("Usage: ryg-rans-rs encode|decode|inspect|trace|compare|bench");
}
