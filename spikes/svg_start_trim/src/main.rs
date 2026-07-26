// Deliberately the *same source* as svg_start_resvg — the only variable under test is
// the feature set in Cargo.toml, so sharing the file guarantees the code cannot drift
// and quietly become a second variable.
include!("../../svg_start_resvg/src/main.rs");
