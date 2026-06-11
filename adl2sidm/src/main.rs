//! Binary entry point for `adl2sidm`.
//!
//! The argument parsing and `.adl` -> `.rs` driver are wired in the CLI commit
//! (Wave C); for now this is the scaffold so the crate builds as a workspace
//! member.

fn main() {
    eprintln!(
        "adl2sidm {}: MEDM .adl -> SiDM (Rust) converter (CLI not wired yet)",
        env!("CARGO_PKG_VERSION")
    );
}
