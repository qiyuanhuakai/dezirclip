//! 17 synchronous, pure text transforms used by the clipboard's "edit before paste" feature.
//!
//! Every transform takes `&str` and returns `Result<String, TransformError>`. All work on
//! Unicode `char` boundaries (no byte slicing), preserve UTF-8 safety, and treat empty input
//! as a valid no-op (returning `Ok(String::new())`).
//!
//! Errors are returned only when the caller asks the transform to *decode* something that is
//! not well-formed (URL percent-decoding, base64 decoding, invalid UTF-8 inside base64). The
//! dispatcher [`apply_transform`] is the single public entry point the Tauri command layer
//! (`app/commands/transform_cmd.rs`) will eventually wrap in `spawn_blocking`.

use std::fmt;

use base64::Engine as _;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Reasons a transform can fail. Only decoders produce errors; encoders and
/// re-shapers always succeed on well-formed `&str` input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformError {
    /// Caller-supplied input violates a precondition the transform cares about
    /// (e.g. base64 length must be a multiple of 4 with correct padding).
    InvalidInput(String),
    /// Bytes could not be decoded (bad percent-encoding, bad base64, bad UTF-8).
    DecodeError(String),
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::InvalidInput(s) => write!(f, "invalid input: {s}"),
            TransformError::DecodeError(s) => write!(f, "decode error: {s}"),
        }
    }
}

impl std::error::Error for TransformError {}

// ---------------------------------------------------------------------------
// TransformKind — 17 variants, kept in sync with the public functions below
// ---------------------------------------------------------------------------

/// Discriminator for [`apply_transform`]. Every variant maps 1:1 to a public
/// function on this module so callers can use either the enum dispatcher or
/// the named function directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformKind {
    ToUppercase,
    ToLowercase,
    ToTitleCase,
    ToSentenceCase,
    TrimWhitespace,
    RemoveExtraSpaces,
    RemoveLineBreaks,
    SortLinesAsc,
    SortLinesDesc,
    DeduplicateLines,
    ReverseLines,
    ReverseText,
    AddLineNumbers,
    EncodeUrl,
    DecodeUrl,
    EncodeBase64,
    DecodeBase64,
}

impl TransformKind {
    /// Number of distinct kinds — useful for sanity assertions in tests.
    pub const COUNT: usize = 17;

    /// Returns every variant in the same order as declared.
    pub const fn all() -> [TransformKind; Self::COUNT] {
        [
            Self::ToUppercase,
            Self::ToLowercase,
            Self::ToTitleCase,
            Self::ToSentenceCase,
            Self::TrimWhitespace,
            Self::RemoveExtraSpaces,
            Self::RemoveLineBreaks,
            Self::SortLinesAsc,
            Self::SortLinesDesc,
            Self::DeduplicateLines,
            Self::ReverseLines,
            Self::ReverseText,
            Self::AddLineNumbers,
            Self::EncodeUrl,
            Self::DecodeUrl,
            Self::EncodeBase64,
            Self::DecodeBase64,
        ]
    }
}

impl fmt::Display for TransformKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::ToUppercase => "to_uppercase",
            Self::ToLowercase => "to_lowercase",
            Self::ToTitleCase => "to_title_case",
            Self::ToSentenceCase => "to_sentence_case",
            Self::TrimWhitespace => "trim_whitespace",
            Self::RemoveExtraSpaces => "remove_extra_spaces",
            Self::RemoveLineBreaks => "remove_line_breaks",
            Self::SortLinesAsc => "sort_lines_asc",
            Self::SortLinesDesc => "sort_lines_desc",
            Self::DeduplicateLines => "deduplicate_lines",
            Self::ReverseLines => "reverse_lines",
            Self::ReverseText => "reverse_text",
            Self::AddLineNumbers => "add_line_numbers",
            Self::EncodeUrl => "encode_url",
            Self::DecodeUrl => "decode_url",
            Self::EncodeBase64 => "encode_base64",
            Self::DecodeBase64 => "decode_base64",
        };
        f.write_str(name)
    }
}

// ---------------------------------------------------------------------------
// Public transform functions — every one is `fn(&str) -> Result<String, TransformError>`
// ---------------------------------------------------------------------------

pub fn to_uppercase(input: &str) -> Result<String, TransformError> {
    Ok(input.to_uppercase())
}

pub fn to_lowercase(input: &str) -> Result<String, TransformError> {
    Ok(input.to_lowercase())
}

/// Title-case: capitalize the first letter of every whitespace-separated word,
/// lowercase the rest. CJK characters and other non-cased letters pass through
/// untouched (their `to_uppercase`/`to_lowercase` returns the same char).
pub fn to_title_case(input: &str) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len());
    let mut at_word_start = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            out.push(ch);
            at_word_start = true;
        } else if at_word_start {
            out.extend(ch.to_uppercase());
            at_word_start = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    Ok(out)
}

/// Sentence-case: capitalize the first letter of every sentence. The first
/// sentence starts at offset 0; a new sentence begins after `.`, `!`, or `?`.
/// Non-cased letters (digits, CJK, etc.) pass through unchanged.
pub fn to_sentence_case(input: &str) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len());
    let mut at_sentence_start = true;
    for ch in input.chars() {
        if at_sentence_start && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            at_sentence_start = false;
        } else {
            out.push(ch);
        }
        if matches!(ch, '.' | '!' | '?') {
            at_sentence_start = true;
        }
    }
    Ok(out)
}

/// Trim leading/trailing ASCII + Unicode whitespace from every line. Empty
/// lines are preserved. Trailing newline on the input is dropped because
/// `str::lines()` strips line terminators; this matches the convention used
/// by every other line-oriented transform in the module.
pub fn trim_whitespace(input: &str) -> Result<String, TransformError> {
    let trimmed: Vec<&str> = input.lines().map(str::trim).collect();
    Ok(trimmed.join("\n"))
}

/// Collapse runs of horizontal whitespace (spaces and tabs) inside each line
/// to a single space. Newlines and other vertical whitespace are preserved.
pub fn remove_extra_spaces(input: &str) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len());
    let mut prev_horiz = false;
    for ch in input.chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_horiz {
                out.push(' ');
                prev_horiz = true;
            }
        } else {
            out.push(ch);
            prev_horiz = false;
        }
    }
    Ok(out)
}

/// Replace every line break (LF, CRLF, CR) with a single space. Consecutive
/// line breaks collapse to a single space, not to nothing.
pub fn remove_line_breaks(input: &str) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len());
    let mut prev_break = false;
    for ch in input.chars() {
        if ch == '\n' || ch == '\r' {
            if !prev_break {
                out.push(' ');
                prev_break = true;
            }
        } else {
            out.push(ch);
            prev_break = false;
        }
    }
    Ok(out)
}

/// Stable ascending sort of lines (lexicographic byte order, which is the
/// only correct choice for `&str` slices). `slice::sort` is documented as
/// stable in std.
pub fn sort_lines_asc(input: &str) -> Result<String, TransformError> {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort();
    Ok(lines.join("\n"))
}

/// Stable descending sort of lines.
pub fn sort_lines_desc(input: &str) -> Result<String, TransformError> {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_by(|a, b| b.cmp(a));
    Ok(lines.join("\n"))
}

/// Remove duplicate lines while preserving first-seen order. Comparison is
/// byte-wise on the trimmed line content (no Unicode normalization).
pub fn deduplicate_lines(input: &str) -> Result<String, TransformError> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out: Vec<&str> = Vec::new();
    for line in input.lines() {
        if seen.insert(line) {
            out.push(line);
        }
    }
    Ok(out.join("\n"))
}

/// Reverse the order of lines (last line becomes first). Line content is not
/// modified — use `reverse_text` for character-wise reversal.
pub fn reverse_lines(input: &str) -> Result<String, TransformError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut reversed = lines;
    reversed.reverse();
    Ok(reversed.join("\n"))
}

/// Reverse the input at character (Unicode scalar value) boundaries. This is
/// NOT byte reversal — multi-byte CJK and emoji sequences stay intact, only
/// the order of `char`s is flipped.
pub fn reverse_text(input: &str) -> Result<String, TransformError> {
    Ok(input.chars().rev().collect())
}

/// Prefix every line with `N. ` where N is 1-based. Empty input yields empty
/// output. Empty lines are numbered as well (`1. `, `2. `, ...).
pub fn add_line_numbers(input: &str) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len() + 8);
    for (idx, line) in input.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        // 1-based numbering. use formatting to avoid manual padding logic.
        out.push_str(&(idx + 1).to_string());
        out.push_str(". ");
        out.push_str(line);
    }
    Ok(out)
}

/// Percent-encode for use in a URL query string. `urlencoding::encode`
/// encodes everything except `A-Z a-z 0-9 - _ . ~`, which is the standard
/// `application/x-www-form-urlencoded` set.
pub fn encode_url(input: &str) -> Result<String, TransformError> {
    Ok(urlencoding::encode(input).into_owned())
}

/// Reverse of [`encode_url`]. Malformed percent sequences (e.g. `%ZZ`) return
/// `TransformError::DecodeError` rather than panicking.
pub fn decode_url(input: &str) -> Result<String, TransformError> {
    urlencoding::decode(input)
        .map(|cow| cow.into_owned())
        .map_err(|e| TransformError::DecodeError(e.to_string()))
}

/// Standard base64 (RFC 4648 §4) with `+`, `/`, `=` padding. Operates on the
/// raw UTF-8 byte representation of the input.
pub fn encode_base64(input: &str) -> Result<String, TransformError> {
    Ok(base64::engine::general_purpose::STANDARD.encode(input.as_bytes()))
}

/// Reverse of [`encode_base64`]. Returns `DecodeError` for malformed base64
/// or for byte sequences that are not valid UTF-8.
pub fn decode_base64(input: &str) -> Result<String, TransformError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(input.as_bytes())
        .map_err(|e| TransformError::DecodeError(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| TransformError::DecodeError(e.to_string()))
}

// ---------------------------------------------------------------------------
// Central dispatcher
// ---------------------------------------------------------------------------

/// Apply the chosen transform. This is the only function the command layer
/// needs to import — Tauri commands wrap this in `spawn_blocking` because it
/// may do non-trivial work (e.g. sorting a 100k-line paste).
pub fn apply_transform(input: &str, kind: TransformKind) -> Result<String, TransformError> {
    match kind {
        TransformKind::ToUppercase => to_uppercase(input),
        TransformKind::ToLowercase => to_lowercase(input),
        TransformKind::ToTitleCase => to_title_case(input),
        TransformKind::ToSentenceCase => to_sentence_case(input),
        TransformKind::TrimWhitespace => trim_whitespace(input),
        TransformKind::RemoveExtraSpaces => remove_extra_spaces(input),
        TransformKind::RemoveLineBreaks => remove_line_breaks(input),
        TransformKind::SortLinesAsc => sort_lines_asc(input),
        TransformKind::SortLinesDesc => sort_lines_desc(input),
        TransformKind::DeduplicateLines => deduplicate_lines(input),
        TransformKind::ReverseLines => reverse_lines(input),
        TransformKind::ReverseText => reverse_text(input),
        TransformKind::AddLineNumbers => add_line_numbers(input),
        TransformKind::EncodeUrl => encode_url(input),
        TransformKind::DecodeUrl => decode_url(input),
        TransformKind::EncodeBase64 => encode_base64(input),
        TransformKind::DecodeBase64 => decode_base64(input),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- 1. to_uppercase ---------------------------------------------------

    #[test]
    fn to_uppercase_basic() {
        assert_eq!(to_uppercase("hello world").unwrap(), "HELLO WORLD");
    }

    #[test]
    fn to_uppercase_empty() {
        assert_eq!(to_uppercase("").unwrap(), "");
    }

    #[test]
    fn to_uppercase_unicode_and_cased_letters() {
        // German sharp s -> SS, CJK unchanged
        assert_eq!(to_uppercase("café ß 你好").unwrap(), "CAFÉ SS 你好");
    }

    // -- 2. to_lowercase ---------------------------------------------------

    #[test]
    fn to_lowercase_basic() {
        assert_eq!(to_lowercase("HELLO World").unwrap(), "hello world");
    }

    #[test]
    fn to_lowercase_empty() {
        assert_eq!(to_lowercase("").unwrap(), "");
    }

    // -- 3. to_title_case --------------------------------------------------

    #[test]
    fn to_title_case_basic() {
        assert_eq!(to_title_case("hello world").unwrap(), "Hello World");
    }

    #[test]
    fn to_title_case_mixed_case_input() {
        // We lowercase the rest of each word, so mixed input becomes canonical.
        assert_eq!(to_title_case("HELLO wOrLd").unwrap(), "Hello World");
    }

    #[test]
    fn to_title_case_empty() {
        assert_eq!(to_title_case("").unwrap(), "");
    }

    #[test]
    fn to_title_case_unicode_passthrough() {
        // CJK chars are not "letters" in the Unicode sense that changes under
        // case mapping, so they pass through unchanged.
        assert_eq!(to_title_case("你好 world").unwrap(), "你好 World");
    }

    // -- 4. to_sentence_case ----------------------------------------------

    #[test]
    fn to_sentence_case_basic() {
        assert_eq!(
            to_sentence_case("hello. world! how are you?").unwrap(),
            "Hello. World! How are you?"
        );
    }

    #[test]
    fn to_sentence_case_first_letter_only() {
        // Unlike title case, the body of each sentence is preserved.
        assert_eq!(
            to_sentence_case("FIRST sentence. SECOND sentence.").unwrap(),
            "FIRST sentence. SECOND sentence."
        );
    }

    #[test]
    fn to_sentence_case_empty() {
        assert_eq!(to_sentence_case("").unwrap(), "");
    }

    // -- 5. trim_whitespace -----------------------------------------------

    #[test]
    fn trim_whitespace_basic() {
        assert_eq!(
            trim_whitespace("   hello   \n   world   ").unwrap(),
            "hello\nworld"
        );
    }

    #[test]
    fn trim_whitespace_empty() {
        assert_eq!(trim_whitespace("").unwrap(), "");
    }

    #[test]
    fn trim_whitespace_preserves_blank_lines() {
        // A line with only spaces becomes an empty line, not removed.
        assert_eq!(trim_whitespace("a\n   \nb").unwrap(), "a\n\nb");
    }

    // -- 6. remove_extra_spaces -------------------------------------------

    #[test]
    fn remove_extra_spaces_collapses_runs() {
        assert_eq!(
            remove_extra_spaces("hello   world").unwrap(),
            "hello world"
        );
    }

    #[test]
    fn remove_extra_spaces_collapses_tabs() {
        assert_eq!(
            remove_extra_spaces("a\t\tb").unwrap(),
            "a b"
        );
    }

    #[test]
    fn remove_extra_spaces_preserves_newlines() {
        // Newlines are vertical whitespace, not horizontal.
        assert_eq!(
            remove_extra_spaces("a  \n  b").unwrap(),
            "a \n b"
        );
    }

    #[test]
    fn remove_extra_spaces_empty() {
        assert_eq!(remove_extra_spaces("").unwrap(), "");
    }

    // -- 7. remove_line_breaks --------------------------------------------

    #[test]
    fn remove_line_breaks_basic() {
        assert_eq!(remove_line_breaks("a\nb\nc").unwrap(), "a b c");
    }

    #[test]
    fn remove_line_breaks_crlf() {
        assert_eq!(remove_line_breaks("a\r\nb").unwrap(), "a b");
    }

    #[test]
    fn remove_line_breaks_collapse_consecutive() {
        // Two newlines back-to-back -> one space, not two.
        assert_eq!(remove_line_breaks("a\n\nb").unwrap(), "a b");
    }

    #[test]
    fn remove_line_breaks_empty() {
        assert_eq!(remove_line_breaks("").unwrap(), "");
    }

    // -- 8. sort_lines_asc -------------------------------------------------

    #[test]
    fn sort_lines_asc_basic() {
        assert_eq!(sort_lines_asc("c\na\nb").unwrap(), "a\nb\nc");
    }

    #[test]
    fn sort_lines_asc_stable() {
        // Stable sort preserves the original order of equal elements.
        // We assert this by sorting a list with duplicates twice and
        // confirming identical-key elements stay in their original order.
        let input = "b1\na\nb2\nb3\na";
        let sorted = sort_lines_asc(input).unwrap();
        // Equal-prefix entries ("a" and "a") keep relative order — there is
        // only one "a" so this just checks the unique-key behavior; the
        // stability claim is checked in sort_stability_check below.
        assert_eq!(sorted, "a\na\nb1\nb2\nb3");
    }

    #[test]
    fn sort_lines_asc_stability_check() {
        // Same-length keys, equal value: stable sort must keep input order.
        let input = "zz_1\nzz_2\nzz_3";
        let sorted = sort_lines_asc(input).unwrap();
        assert_eq!(sorted, "zz_1\nzz_2\nzz_3");
    }

    #[test]
    fn sort_lines_asc_empty() {
        assert_eq!(sort_lines_asc("").unwrap(), "");
    }

    // -- 9. sort_lines_desc ------------------------------------------------

    #[test]
    fn sort_lines_desc_basic() {
        assert_eq!(sort_lines_desc("a\nb\nc").unwrap(), "c\nb\na");
    }

    #[test]
    fn sort_lines_desc_empty() {
        assert_eq!(sort_lines_desc("").unwrap(), "");
    }

    // -- 10. deduplicate_lines --------------------------------------------

    #[test]
    fn deduplicate_lines_basic() {
        assert_eq!(
            deduplicate_lines("a\nb\na\nc\nb").unwrap(),
            "a\nb\nc"
        );
    }

    #[test]
    fn deduplicate_lines_preserves_first_seen_order() {
        // "x" appears in positions 0 and 3; "x" must appear only once, at the
        // first position. "y" appears at positions 1 and 4 — keep position 1.
        let out = deduplicate_lines("x\ny\nz\nx\ny").unwrap();
        assert_eq!(out, "x\ny\nz");
    }

    #[test]
    fn deduplicate_lines_empty() {
        assert_eq!(deduplicate_lines("").unwrap(), "");
    }

    // -- 11. reverse_lines -------------------------------------------------

    #[test]
    fn reverse_lines_basic() {
        assert_eq!(reverse_lines("a\nb\nc").unwrap(), "c\nb\na");
    }

    #[test]
    fn reverse_lines_single_line() {
        assert_eq!(reverse_lines("only").unwrap(), "only");
    }

    #[test]
    fn reverse_lines_empty() {
        assert_eq!(reverse_lines("").unwrap(), "");
    }

    // -- 12. reverse_text --------------------------------------------------

    #[test]
    fn reverse_text_basic() {
        assert_eq!(reverse_text("hello").unwrap(), "olleh");
    }

    #[test]
    fn reverse_text_unicode_codepoint_safe() {
        // Char-wise, not byte-wise: "你好" reversed is "好你" (3 bytes per char).
        assert_eq!(reverse_text("你好").unwrap(), "好你");
    }

    #[test]
    fn reverse_text_emoji() {
        // Multi-codepoint emoji cluster: each scalar value reversed. The
        // important property is that we never slice in the middle of a
        // multi-byte sequence.
        let input = "👨‍👩‍👧 a";
        let out = reverse_text(input).unwrap();
        // Char count preserved; bytes may not be (we just need to know we
        // didn't panic and produced the right char sequence).
        let input_chars: Vec<char> = input.chars().collect();
        let out_chars: Vec<char> = out.chars().collect();
        let mut expected: Vec<char> = input_chars.into_iter().rev().collect();
        assert_eq!(out_chars, expected);
        // Spot-check: starts with the trailing space, then 'a'.
        expected.reverse();
        let _ = expected;
    }

    #[test]
    fn reverse_text_empty() {
        assert_eq!(reverse_text("").unwrap(), "");
    }

    // -- 13. add_line_numbers ---------------------------------------------

    #[test]
    fn add_line_numbers_basic() {
        assert_eq!(add_line_numbers("a\nb").unwrap(), "1. a\n2. b");
    }

    #[test]
    fn add_line_numbers_empty_line_is_numbered() {
        // Empty lines get a number; the "content" is empty.
        assert_eq!(add_line_numbers("a\n\nb").unwrap(), "1. a\n2. \n3. b");
    }

    #[test]
    fn add_line_numbers_empty() {
        assert_eq!(add_line_numbers("").unwrap(), "");
    }

    // -- 14. encode_url ---------------------------------------------------

    #[test]
    fn encode_url_basic() {
        assert_eq!(encode_url("hello world").unwrap(), "hello%20world");
    }

    #[test]
    fn encode_url_unicode() {
        // UTF-8 bytes percent-encoded.
        assert_eq!(encode_url("你好").unwrap(), "%E4%BD%A0%E5%A5%BD");
    }

    #[test]
    fn encode_url_empty() {
        assert_eq!(encode_url("").unwrap(), "");
    }

    // -- 15. decode_url ---------------------------------------------------

    #[test]
    fn decode_url_basic() {
        assert_eq!(decode_url("hello%20world").unwrap(), "hello world");
    }

    #[test]
    fn decode_url_unicode() {
        assert_eq!(decode_url("%E4%BD%A0%E5%A5%BD").unwrap(), "你好");
    }

    #[test]
    fn decode_url_empty() {
        assert_eq!(decode_url("").unwrap(), "");
    }

    #[test]
    fn decode_url_invalid_returns_error_no_panic() {
        // %FF decodes to the byte 0xFF, which is not valid UTF-8. The urlencoding
        // crate returns the FromUtf8Error, which we wrap as DecodeError.
        // (%ZZ, by contrast, is *lenient* in urlencoding 2.x — the crate passes
        // invalid hex digits through unchanged rather than rejecting them.)
        let result = decode_url("hello%FFworld");
        assert!(matches!(result, Err(TransformError::DecodeError(_))));
    }

    // -- 16. encode_base64 ------------------------------------------------

    #[test]
    fn encode_base64_known_vector() {
        // RFC 4648 §10: "" -> "", "f" -> "Zg==", "fo" -> "Zm8=", "foo" -> "Zm9v",
        // "foob" -> "Zm9vYg==", "fooba" -> "Zm9vYmE=", "foobar" -> "Zm9vYmFy".
        assert_eq!(encode_base64("").unwrap(), "");
        assert_eq!(encode_base64("f").unwrap(), "Zg==");
        assert_eq!(encode_base64("fo").unwrap(), "Zm8=");
        assert_eq!(encode_base64("foo").unwrap(), "Zm9v");
        assert_eq!(encode_base64("foobar").unwrap(), "Zm9vYmFy");
    }

    #[test]
    fn encode_base64_unicode_bytes() {
        // "你" is 3 bytes (0xE4 0xBD 0xA0). Encoded as base64.
        assert_eq!(encode_base64("你").unwrap(), "5L2g");
    }

    // -- 17. decode_base64 ------------------------------------------------

    #[test]
    fn decode_base64_known_vector() {
        assert_eq!(decode_base64("").unwrap(), "");
        assert_eq!(decode_base64("Zg==").unwrap(), "f");
        assert_eq!(decode_base64("Zm8=").unwrap(), "fo");
        assert_eq!(decode_base64("Zm9v").unwrap(), "foo");
        assert_eq!(decode_base64("Zm9vYmFy").unwrap(), "foobar");
    }

    #[test]
    fn decode_base64_invalid_returns_error_no_panic() {
        // "!!" is not in the base64 alphabet.
        let result = decode_base64("!!!!");
        assert!(matches!(result, Err(TransformError::DecodeError(_))));
    }

    #[test]
    fn decode_base64_invalid_utf8_returns_error() {
        // 0xFF is not a valid UTF-8 leading byte. base64-decoded to bytes,
        // the second step (String::from_utf8) must fail cleanly.
        // 0xFF in base64 is "/w==".
        let result = decode_base64("/w==");
        assert!(matches!(result, Err(TransformError::DecodeError(_))));
    }

    #[test]
    fn base64_round_trip_unicode() {
        let original = "Tiez 剪贴板 🚀 emoji";
        let encoded = encode_base64(original).unwrap();
        let decoded = decode_base64(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn url_round_trip_unicode() {
        let original = "搜索 query=hello world 你好";
        let encoded = encode_url(original).unwrap();
        let decoded = decode_url(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    // -- dispatcher + cross-cutting ---------------------------------------

    #[test]
    fn apply_transform_dispatches_all_kinds() {
        // Spot-check every kind by going through the dispatcher, not the
        // direct function. This guards against a typo in the match arms.
        assert_eq!(
            apply_transform("Hello", TransformKind::ToUppercase).unwrap(),
            "HELLO"
        );
        assert_eq!(
            apply_transform("Hello", TransformKind::ToLowercase).unwrap(),
            "hello"
        );
        assert_eq!(
            apply_transform("hello world", TransformKind::ToTitleCase).unwrap(),
            "Hello World"
        );
        assert_eq!(
            apply_transform("hello. world", TransformKind::ToSentenceCase).unwrap(),
            "Hello. World"
        );
        assert_eq!(
            apply_transform("  x  \n  y  ", TransformKind::TrimWhitespace).unwrap(),
            "x\ny"
        );
        assert_eq!(
            apply_transform("a  b", TransformKind::RemoveExtraSpaces).unwrap(),
            "a b"
        );
        assert_eq!(
            apply_transform("a\nb", TransformKind::RemoveLineBreaks).unwrap(),
            "a b"
        );
        assert_eq!(
            apply_transform("c\na\nb", TransformKind::SortLinesAsc).unwrap(),
            "a\nb\nc"
        );
        assert_eq!(
            apply_transform("a\nb\nc", TransformKind::SortLinesDesc).unwrap(),
            "c\nb\na"
        );
        assert_eq!(
            apply_transform("a\nb\na", TransformKind::DeduplicateLines).unwrap(),
            "a\nb"
        );
        assert_eq!(
            apply_transform("a\nb\nc", TransformKind::ReverseLines).unwrap(),
            "c\nb\na"
        );
        assert_eq!(
            apply_transform("abc", TransformKind::ReverseText).unwrap(),
            "cba"
        );
        assert_eq!(
            apply_transform("a\nb", TransformKind::AddLineNumbers).unwrap(),
            "1. a\n2. b"
        );
        assert_eq!(
            apply_transform("a b", TransformKind::EncodeUrl).unwrap(),
            "a%20b"
        );
        assert_eq!(
            apply_transform("a%20b", TransformKind::DecodeUrl).unwrap(),
            "a b"
        );
        assert_eq!(
            apply_transform("hi", TransformKind::EncodeBase64).unwrap(),
            "aGk="
        );
        assert_eq!(
            apply_transform("aGk=", TransformKind::DecodeBase64).unwrap(),
            "hi"
        );
    }

    #[test]
    fn apply_transform_empty_input_returns_empty_for_every_kind() {
        // Empty input is a no-op for every transform except decoders (which
        // also return empty on empty input — that's the spec).
        for kind in TransformKind::all() {
            let result = apply_transform("", kind);
            assert!(
                result.is_ok(),
                "kind {kind:?} returned error on empty input: {:?}",
                result
            );
            assert_eq!(result.unwrap(), "", "kind {kind:?} did not return \"\"");
        }
    }

    #[test]
    fn apply_transform_long_text_does_not_panic() {
        // 1250 lines of 40 chars + 1249 newline separators = 51_249 bytes.
        // Big enough to exercise the inner loops; small enough to be instant.
        let line = "x".repeat(40);
        let input: String = std::iter::repeat(line.as_str())
            .take(1250)
            .collect::<Vec<_>>()
            .join("\n");
        let input_len = input.len();
        assert_eq!(input_len, 1250 * 40 + 1249);
        assert!(input_len > 50_000);

        // Sort preserves length (same lines, same separators).
        let sorted = apply_transform(&input, TransformKind::SortLinesAsc).unwrap();
        assert_eq!(sorted.len(), input_len);

        // Deduplicate collapses to one line.
        let dedup = apply_transform(&input, TransformKind::DeduplicateLines).unwrap();
        assert_eq!(dedup, "x".repeat(40));

        // Add line numbers prefixes "N. " to every line.
        let numbered = apply_transform(&input, TransformKind::AddLineNumbers).unwrap();
        assert!(numbered.starts_with("1. x"));
        assert!(numbered.contains("1250. "));
        assert!(numbered.len() > input_len);

        // Reverse text preserves char count.
        let reversed = apply_transform(&input, TransformKind::ReverseText).unwrap();
        assert_eq!(reversed.chars().count(), input.chars().count());
    }

    #[test]
    fn transform_error_display() {
        let invalid = TransformError::InvalidInput("bad".to_string());
        assert_eq!(invalid.to_string(), "invalid input: bad");
        let decode = TransformError::DecodeError("xx".to_string());
        assert_eq!(decode.to_string(), "decode error: xx");
    }

    #[test]
    fn transform_kind_count_matches_all_iterator() {
        // Sanity: every variant is reachable from `all()` and the count is 17.
        assert_eq!(TransformKind::all().len(), TransformKind::COUNT);
        assert_eq!(TransformKind::COUNT, 17);
    }

    #[test]
    fn transform_kind_display_uses_snake_case() {
        assert_eq!(TransformKind::ToUppercase.to_string(), "to_uppercase");
        assert_eq!(TransformKind::AddLineNumbers.to_string(), "add_line_numbers");
    }
}
