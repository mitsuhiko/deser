//! Enables the capabilities of the template (see `generate.py`).
//!
//! The template compiles as JSON5, the dialect with the most capabilities,
//! so that the code of all capabilities is checked and analyzed by editors.
//! JSON5 and Hjson differ in how they read unquoted values and keys, a
//! dialect can only be one of them.  The code of Hjson is checked by
//! compiling `deser-hjson`.
fn main() {
    for cap in [
        "comments",
        "trailing_commas",
        "single_quotes",
        "json5",
        "hjson",
    ] {
        println!("cargo::rustc-check-cfg=cfg({cap})");
    }
    for cap in ["comments", "trailing_commas", "single_quotes", "json5"] {
        println!("cargo::rustc-cfg={cap}");
    }
}
