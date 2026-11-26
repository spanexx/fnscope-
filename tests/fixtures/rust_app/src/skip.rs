//! A normal source module whose name must not trigger any default exclusion.

/// Returns its input unchanged; proves ordinary `src/*.rs` files are scanned.
pub fn passthrough(value: i32) -> i32 {
    value
}
