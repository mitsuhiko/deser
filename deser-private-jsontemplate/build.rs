//! Enables all capabilities of the template (see `generate.py`).
//!
//! The template compiles as the dialect with the most features so that the
//! code of all capabilities is checked and analyzed by editors.
fn main() {
    for cap in ["comments", "trailing_commas", "json5"] {
        println!("cargo::rustc-check-cfg=cfg({cap})");
        println!("cargo::rustc-cfg={cap}");
    }
}
