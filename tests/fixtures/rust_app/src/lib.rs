//! A small, coherent Rust crate used as fnscope's end-to-end Rust fixture.
//!
//! It mirrors the shape of the TypeScript/JavaScript fixture: free functions,
//! an impl method with a downstream call edge, a guarded fallible function with
//! a `match`, and a side-effecting function.

pub mod skip;

/// Adds two signed integers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Scales values by a fixed integer factor.
pub struct Calculator {
    factor: i32,
}

impl Calculator {
    /// Builds a calculator with the given scaling factor.
    pub fn new(factor: i32) -> Self {
        Self { factor }
    }

    /// Scales `value` by this calculator's factor via repeated `add` calls.
    pub fn scale(&self, value: i32) -> i32 {
        let mut result = 0;
        for _ in 0..self.factor.abs() {
            result = add(result, value);
        }
        if self.factor < 0 {
            -result
        } else {
            result
        }
    }
}

/// The grammatical role assigned to a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// The text performs the action.
    Doer,
    /// The text receives the action.
    Receiver,
}

/// A stable error code returned by fallible fixture functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    /// Machine-readable code, for example `EMPTY_TEXT`.
    pub code: &'static str,
}

/// Lowercases and trims text so classification is case-insensitive.
pub fn normalize_text(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// Classifies text as a [`Label`], rejecting empty input with `EMPTY_TEXT`.
pub fn classify(text: &str) -> Result<Label, AppError> {
    if text.is_empty() {
        return Err(AppError { code: "EMPTY_TEXT" });
    }

    let normalized = normalize_text(text);
    let label = match normalized.ends_with("er") {
        true => Label::Doer,
        false => Label::Receiver,
    };

    Ok(label)
}

/// Prints a human-readable report, providing a side-effect example.
pub fn report(text: &str) {
    println!("report: {}", text);
}
