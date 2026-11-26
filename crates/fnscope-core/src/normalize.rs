//! Formatting-insensitive source normalization and stable identifier helpers.

use sha2::{Digest, Sha256};

/// States of the normalization scanner; quoted sections are copied verbatim.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    LineComment,
    /// Nesting depth: Rust block comments may contain further block comments.
    BlockComment(usize),
    SingleQuote,
    DoubleQuote,
    Template,
    /// Rust raw string (`r"..."`, `r#"..."#`); value is the trailing `#` count.
    RawString(usize),
}

/// Whether a character can start a Rust identifier (and therefore a lifetime).
fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

/// Whether a character can continue a Rust identifier.
fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_numeric()
}

/// Whether a `'` at this position opens a Rust lifetime rather than a literal.
///
/// A lifetime is `'` + identifier without a closing `'` (`'a`, `'static`, `'_`);
/// `'a'` and `'\n'` are character literals. The lookahead works on a clone so
/// the real scanner cursor is never advanced just to classify a quote.
fn is_lifetime(chars: &std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let mut look = chars.clone();
    let Some(first) = look.next() else {
        return false;
    };
    if !is_ident_start(first) {
        return false;
    }
    while let Some(&next) = look.peek() {
        if is_ident_continue(next) {
            look.next();
        } else {
            break;
        }
    }
    look.peek() != Some(&'\'')
}

/// Whether the source contains a declaration marker unique to Rust.
///
/// TS/JS use `=>` rather than `->` and have no `fn`, `impl` or `::` syntax, so
/// limiting lifetime handling to these sources keeps quoted TS/JS strings
/// (for example `'hello world'`) on their original, literal-preserving path.
/// Function slices emitted by discovery always contain `fn `, so every hashed
/// Rust function gets the lifetime-aware normalization.
fn looks_like_rust(source: &str) -> bool {
    ["fn ", "->", "::", "impl "]
        .iter()
        .any(|marker| source.contains(marker))
}

/// Emits a buffered terminator before a token that is not a block boundary.
fn flush_semicolon(out: &mut String, pending: &mut bool) {
    if *pending {
        out.push(';');
        *pending = false;
    }
}

/// Strips comments and whitespace so that formatting-only edits hash identically.
///
/// Single-quoted, double-quoted, template-literal and Rust raw-string spans are
/// preserved byte for byte. Template literals are deliberately treated as opaque,
/// so `${...}` sub-expressions are kept verbatim instead of being normalized
/// separately. Rust block comments are nested and Rust lifetimes (`'a`) are code,
/// not quote openers. A semicolon directly before `}` or end of input is also
/// treated as optional formatting; semicolons inside expressions are preserved.
pub fn normalize_source(source: &str) -> String {
    let rust_lifetimes = looks_like_rust(source);
    let mut mode = Mode::Code;
    let mut out = String::with_capacity(source.len());
    let mut pending_semicolon = false;
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        match mode {
            Mode::Code => match ch {
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    mode = Mode::LineComment;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    mode = Mode::BlockComment(1);
                }
                '\'' if rust_lifetimes && is_lifetime(&chars) => {
                    // A lifetime is code: emit only the quote and let its
                    // identifier and any following whitespace follow the Code
                    // rules, so reformatting a generic signature stays stable.
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(ch);
                }
                '\'' => {
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(ch);
                    mode = Mode::SingleQuote;
                }
                '"' => {
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(ch);
                    mode = Mode::DoubleQuote;
                }
                '`' => {
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(ch);
                    mode = Mode::Template;
                }
                'r' => {
                    // Rust raw strings (`r"..."`, `r#"..."#`, `br#"..."#`) have
                    // no escapes: an inner quote is ordinary content. Detect the
                    // prefix here; `r#ident` raw identifiers fall through as code.
                    let mut hashes = 0usize;
                    while chars.peek() == Some(&'#') {
                        chars.next();
                        hashes += 1;
                    }
                    let is_raw = chars.peek() == Some(&'"');
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(ch);
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    if is_raw {
                        out.push(chars.next().expect("peeked opening quote"));
                        mode = Mode::RawString(hashes);
                    }
                }
                ';' => {
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    pending_semicolon = true;
                }
                c if c.is_whitespace() => {}
                '}' => {
                    pending_semicolon = false;
                    out.push('}');
                }
                c => {
                    flush_semicolon(&mut out, &mut pending_semicolon);
                    out.push(c);
                }
            },
            Mode::LineComment => {
                if ch == '\n' || ch == '\r' {
                    mode = Mode::Code;
                }
            }
            Mode::BlockComment(depth) => {
                if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    mode = Mode::BlockComment(depth + 1);
                } else if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    if depth == 1 {
                        mode = Mode::Code;
                    } else {
                        mode = Mode::BlockComment(depth - 1);
                    }
                }
            }
            Mode::SingleQuote | Mode::DoubleQuote | Mode::Template => {
                out.push(ch);
                if ch == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                } else {
                    let closes = matches!(
                        (mode, ch),
                        (Mode::SingleQuote, '\'')
                            | (Mode::DoubleQuote, '"')
                            | (Mode::Template, '`')
                    );
                    if closes {
                        mode = Mode::Code;
                    }
                }
            }
            Mode::RawString(hashes) => {
                out.push(ch);
                if ch == '"' {
                    // The delimiter is `"` followed by exactly `hashes` '#'.
                    // Any other characters are raw content and stay verbatim.
                    let mut closed = true;
                    for _ in 0..hashes {
                        match chars.next() {
                            Some('#') => out.push('#'),
                            Some(other) => {
                                out.push(other);
                                closed = false;
                                break;
                            }
                            None => {
                                closed = false;
                                break;
                            }
                        }
                    }
                    if closed {
                        mode = Mode::Code;
                    }
                }
            }
        }
    }

    out
}

/// Returns a stable, hex-encoded SHA-256 digest of the normalized source.
pub fn ast_hash(source: &str) -> String {
    let digest = Sha256::digest(normalize_source(source).as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

/// Turns a qualified function name into a stable kebab-case identifier.
///
/// The relative path is accepted for symmetry with the scanner; module-based
/// disambiguation of collisions is performed there, not here.
pub fn make_id(relative_path: &str, qualified_name: &str) -> String {
    let _ = relative_path;

    let chars: Vec<char> = qualified_name.chars().collect();
    let mut spaced = String::with_capacity(qualified_name.len());
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && index > 0 {
            let previous = chars[index - 1];
            let next = chars.get(index + 1).copied();
            let boundary = previous.is_lowercase()
                || previous.is_ascii_digit()
                || (previous.is_uppercase() && next.is_some_and(|n| n.is_lowercase()));
            if boundary {
                spaced.push('-');
            }
        }
        spaced.push(*ch);
    }

    let mut slug = String::with_capacity(spaced.len());
    let mut last_was_dash = false;
    for ch in spaced.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_ignores_formatting_and_optional_semicolons() {
        let left = normalize_source("function f( a , b ){ return a+b; }");
        let right = normalize_source("function f(a,b){return a+b}");
        assert_eq!(left, right);
        assert_eq!(left, "functionf(a,b){returna+b}");
    }

    #[test]
    fn normalization_preserves_literals_and_removes_comments() {
        let normalized = normalize_source(
            "let a = 1; // trailing comment\n/* block\n comment */\n\
             const b = \"// not a comment\"; const c = `x  y // ${a}`;",
        );
        assert!(!normalized.contains("trailing"));
        assert!(!normalized.contains("block"));
        assert!(normalized.contains("\"// not a comment\""));
        assert!(normalized.contains("`x  y // ${a}`"));
        assert!(normalized.contains("leta=1;constb"));
    }

    #[test]
    fn normalization_keeps_expression_semicolons() {
        let normalized = normalize_source("for (let i = 0; i < 3; i++) {}");
        assert_eq!(normalized, "for(leti=0;i<3;i++){}");
    }

    #[test]
    fn ast_hash_is_prefixed_hex_and_formatting_stable() {
        let hash = ast_hash("function f( a , b ){ return a+b; }");
        assert!(hash.starts_with("sha256:"));
        let digest = &hash["sha256:".len()..];
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, ast_hash("function f(a,b){return a+b}"));
        assert_ne!(hash, ast_hash("function f(a,b){return a-b}"));
    }

    #[test]
    fn make_id_follows_kebab_case_contract() {
        assert_eq!(
            make_id("nlp/roles.ts", "evaluateGrammarRole"),
            "evaluate-grammar-role"
        );
        assert_eq!(make_id("a/B.ts", "XMLParser"), "xml-parser");
        assert_eq!(make_id("x.ts", "Formatter.format"), "formatter-format");
        assert_eq!(
            make_id("x.ts", "dispatchWebhookEvent"),
            "dispatch-webhook-event"
        );
        assert_eq!(make_id("x.ts", "snake_case!!"), "snake-case");
    }

    #[test]
    fn rust_formatting_only_changes_keep_the_same_hash() {
        let spaced =
            "pub fn greet<'a>(name: &'a str) -> String { let s = r#\"hi  there\"#; format!(\"{}\", s) }";
        let compact =
            "pub fn greet<'a>(name:&'a str)->String{let s=r#\"hi  there\"#;format!(\"{}\",s)}";
        assert_eq!(normalize_source(spaced), normalize_source(compact));
        assert_eq!(ast_hash(spaced), ast_hash(compact));
        // A different raw-string value is a semantic change, not formatting.
        assert_ne!(
            ast_hash(spaced),
            ast_hash(&compact.replace("hi  there", "hi there"))
        );
    }

    #[test]
    fn rust_lifetimes_are_code_not_quote_openers() {
        let spaced = "fn bound<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str { x }";
        let compact = "fn bound<'a,'b:'a>(x:&'a str,y:&'b str)->&'a str{x}";
        let normalized = normalize_source(spaced);
        assert!(normalized.contains("<'a,'b:'a>(x:&'astr"), "{normalized}");
        assert!(!normalized.contains("'a, 'b"), "{normalized}");
        assert_eq!(normalized, normalize_source(compact));
    }

    #[test]
    fn rust_raw_strings_keep_embedded_quotes_and_content() {
        let normalized = normalize_source("fn f() -> &str { r#\"a \" b\"# }");
        assert!(normalized.contains("r#\"a \" b\"#"), "{normalized}");
        assert_ne!(
            normalized,
            normalize_source("fn f() -> &str { r#\"a \"  b\"# }")
        );
        // `r#ident` is a raw identifier, not a raw string prefix.
        assert!(normalize_source("fn f(r#type: i32) -> i32 { r#type }").contains("r#type"));
    }

    #[test]
    fn rust_nested_block_comments_are_fully_stripped() {
        let commented = normalize_source("fn f() { /* outer /* inner */ still */ return 1; }");
        assert_eq!(commented, normalize_source("fn f(){return 1;}"));
    }

    #[test]
    fn typescript_single_quoted_strings_with_spaces_stay_literals() {
        let normalized = normalize_source("const greeting = 'hello world';");
        assert!(normalized.contains("'hello world'"), "{normalized}");
    }
}
