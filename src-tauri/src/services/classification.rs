//! Heuristic content classification for clipboard entries.
//!
//! Twelve classifier functions take `&str` and return `bool`:
//!
//! * [`is_url`]         — `http://`, `https://`, `ftp://`, or `www.` prefix
//! * [`is_email`]       — RFC 5322 simplified: `local@domain.tld`
//! * [`is_phone`]       — Chinese mobile (1[3-9]\d{9}) or E.164 international
//! * [`is_idcard`]      — 18-digit Chinese ID card; last char may be X or x
//! * [`is_ipv4`]        — IPv4 dotted quad, each octet in 0..=255
//! * [`is_ipv6`]        — IPv6 full / compressed form (incl. `::`)
//! * [`is_jwt`]         — three dot-separated base64url segments
//! * [`is_path_windows`]— `C:\` drive letter or `\\server\share` UNC
//! * [`is_path_unix`]   — `/abs/path` or `~/relative`
//! * [`is_color_hex`]   — `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`
//! * [`is_color_rgb`]   — `rgb(...)` or `rgba(...)` with 0..=255 channels
//! * [`is_json`]        — starts with `{` or `[` and parses as JSON
//!
//! The [`classify`] dispatcher runs every classifier and returns the names
//! of all matching kinds. Classification is **pure**: no I/O, no platform
//! code, no shared state. Per-call regex compilation is cheap for short
//! strings; the 5000-entry backfill target in roadmap-2026 §4.4 stays
//! under 2s without `once_cell` caches (which AGENTS.md prohibits).
//!
//! Per G21 there is no trait abstraction: dispatch is a flat `match` over
//! [`CONTENT_KINDS`].

use regex::Regex;

/// All content kinds recognised by [`classify`], in dispatcher order.
///
/// Order is stable so `classify` output can be diffed and stored as CSV
/// in the `content_kinds` column (migration v13, Task 28).
pub const CONTENT_KINDS: &[&str] = &[
    "url",
    "email",
    "phone",
    "idcard",
    "ipv4",
    "ipv6",
    "jwt",
    "path_windows",
    "path_unix",
    "color_hex",
    "color_rgb",
    "json",
];

// ---------------------------------------------------------------------------
// Individual classifiers
// ---------------------------------------------------------------------------

/// URL: `http://`, `https://`, `ftp://`, or `www.` prefix.
pub fn is_url(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    match Regex::new(r"^(?:https?|ftp)://|^www\.") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// RFC 5322 simplified: `local@domain.tld` with a 2+ character TLD.
pub fn is_email(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    match Regex::new(r"^[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}$") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// Chinese mobile (1[3-9]\d{9}, 11 digits) or E.164 international.
///
/// E.164 form: `+\d{1,3}[\s-]?\d{4,14}` (optional country code up to 3
/// digits, optional single space or hyphen, 4–14 digit subscriber).
pub fn is_phone(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let cn = Regex::new(r"^1[3-9]\d{9}$").map(|re| re.is_match(trimmed));
    if matches!(cn, Ok(true)) {
        return true;
    }
    match Regex::new(r"^\+\d{1,3}[\s\-]?\d{4,14}$") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// 18-digit Chinese ID card; the last character may be `X` or `x`
/// (the standard check code).
pub fn is_idcard(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    match Regex::new(r"^\d{17}[\dXx]$") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// IPv4 dotted quad, each octet in `0..=255`.
///
/// A bare regex would accept `999.999.999.999`, so the octet range is
/// validated after the shape match.
pub fn is_ipv4(text: &str) -> bool {
    let trimmed = text.trim();
    let re = match Regex::new(r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$") {
        Ok(re) => re,
        Err(_) => return false,
    };
    let caps = match re.captures(trimmed) {
        Some(c) => c,
        None => return false,
    };
    for i in 1..=4 {
        let m = match caps.get(i) {
            Some(m) => m,
            None => return false,
        };
        match m.as_str().parse::<u32>() {
            Ok(n) if n <= 255 => continue,
            _ => return false,
        }
    }
    true
}

/// IPv6 — full 8-group form or compressed `::` form, with optional
/// zone identifier (`%eth0`).
///
/// Validates: only hex digits + colons + optional `%zone`; a `::` is
/// present or there are exactly 8 groups; every group is 1–4 hex
/// digits; total group count after expanding `::` is `<= 8`. Triple
/// (or more) consecutive colons are explicitly rejected.
pub fn is_ipv6(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.contains(':') {
        return false;
    }
    // Zone identifier: keep only the part before '%'.
    let core = trimmed.split('%').next().unwrap_or(trimmed);
    if core.is_empty() {
        return false;
    }
    if !core.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return false;
    }
    if core.matches(':').count() < 2 {
        return false;
    }
    if core.contains(":::") {
        return false;
    }
    let has_double_colon = core.contains("::");
    if has_double_colon {
        let mut parts = core.splitn(2, "::");
        let left = parts.next().unwrap_or("");
        let right = parts.next().unwrap_or("");
        let left_groups: Vec<&str> = left.split(':').filter(|s| !s.is_empty()).collect();
        let right_groups: Vec<&str> = right.split(':').filter(|s| !s.is_empty()).collect();
        let total = left_groups.len() + right_groups.len();
        if total > 7 {
            return false;
        }
        for g in left_groups.iter().chain(right_groups.iter()) {
            if g.is_empty() || g.len() > 4 {
                return false;
            }
            if !g.chars().all(|c| c.is_ascii_hexdigit()) {
                return false;
            }
        }
        true
    } else {
        let groups: Vec<&str> = core.split(':').collect();
        if groups.len() != 8 {
            return false;
        }
        groups
            .iter()
            .all(|g| !g.is_empty() && g.len() <= 4 && g.chars().all(|c| c.is_ascii_hexdigit()))
    }
}

/// JWT-like three-segment base64url token: `xxx.yyy.zzz`.
///
/// Each segment must be non-empty and contain only `[A-Za-z0-9_-]`. To
/// reduce false positives on very short dot-separated triples
/// (e.g. `1.2.3`), every segment must be at least 4 characters.
pub fn is_jwt(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let re = match Regex::new(r"^[A-Za-z0-9_\-]+$") {
        Ok(re) => re,
        Err(_) => return false,
    };
    if !parts.iter().all(|p| !p.is_empty() && re.is_match(p)) {
        return false;
    }
    parts.iter().all(|p| p.len() >= 4)
}

/// Windows path: `C:\...` (or `C:/...`) drive letter, or `\\server\share`
/// UNC path. Forward and back slashes are accepted in the drive form.
pub fn is_path_windows(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let drive_re = Regex::new(
        r#"^[A-Za-z]:[\\/][^<>:"|?*\r\n]*$"#,
    );
    if let Ok(re) = drive_re {
        if re.is_match(trimmed) {
            return true;
        }
    }
    let unc_re = Regex::new(
        r#"^\\\\[^<>:"|?*\r\n]+[\\/][^<>:"|?*\r\n]+(?:[\\/][^<>:"|?*\r\n]+)*$"#,
    );
    if let Ok(re) = unc_re {
        if re.is_match(trimmed) {
            return true;
        }
    }
    false
}

/// Unix path: absolute `/path/...` or home-relative `~/path/...`.
///
/// Bare `/` is rejected (too short to be a real path); at least one
/// non-empty segment is required.
pub fn is_path_unix(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "~" || trimmed.starts_with("~/") {
        return true;
    }
    if !trimmed.starts_with('/') {
        return false;
    }
    match Regex::new(r"^/(?:[^/]+/)*(?:[^/]+|/)$") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// CSS-style hex color: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
///
/// 3, 4, 6, or 8 hex digits, no other lengths.
pub fn is_color_hex(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    match Regex::new(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$") {
        Ok(re) => re.is_match(trimmed),
        Err(_) => false,
    }
}

/// `rgb(r, g, b)` or `rgba(r, g, b, a)` color literal, with each channel
/// in `0..=255` and an optional alpha (`0..=1` decimal or `0%..=100%`).
///
/// `rgb()` requires exactly 3 channels (no alpha); `rgba()` requires
/// exactly 4 (alpha mandatory).
pub fn is_color_rgb(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let rgb_re = match Regex::new(
        r"^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$",
    ) {
        Ok(re) => re,
        Err(_) => return false,
    };
    if let Some(caps) = rgb_re.captures(trimmed) {
        for i in 1..=3 {
            if let Some(m) = caps.get(i) {
                if let Ok(n) = m.as_str().parse::<u32>() {
                    if n > 255 {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        return true;
    }
    let rgba_re = match Regex::new(
        r"^rgba\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*([\d.]+%?)\s*\)$",
    ) {
        Ok(re) => re,
        Err(_) => return false,
    };
    let caps = match rgba_re.captures(trimmed) {
        Some(c) => c,
        None => return false,
    };
    for i in 1..=3 {
        if let Some(m) = caps.get(i) {
            if let Ok(n) = m.as_str().parse::<u32>() {
                if n > 255 {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    if let Some(alpha) = caps.get(4) {
        let s = alpha.as_str();
        if let Some(pct) = s.strip_suffix('%') {
            match pct.parse::<u32>() {
                Ok(n) if n <= 100 => {}
                _ => return false,
            }
        } else {
            match s.parse::<f64>() {
                Ok(a) if (0.0..=1.0).contains(&a) => {}
                _ => return false,
            }
        }
    } else {
        return false;
    }
    true
}

/// JSON: must start with `{` or `[` after trimming, and parse cleanly
/// via `serde_json`. The leading-character gate avoids claiming prose
/// like "this sentence { contains a brace" as JSON.
pub fn is_json(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Run every classifier in [`CONTENT_KINDS`] order and return the names
/// of all matches. Empty input yields an empty vector (caller decides
/// what to do with the no-kind case; the older `Empty` enum variant from
/// roadmap-2026 §4.4 was removed to keep the public surface flat).
pub fn classify(content: &str) -> Vec<String> {
    let checks: &[(&str, fn(&str) -> bool)] = &[
        ("url", is_url),
        ("email", is_email),
        ("phone", is_phone),
        ("idcard", is_idcard),
        ("ipv4", is_ipv4),
        ("ipv6", is_ipv6),
        ("jwt", is_jwt),
        ("path_windows", is_path_windows),
        ("path_unix", is_path_unix),
        ("color_hex", is_color_hex),
        ("color_rgb", is_color_rgb),
        ("json", is_json),
    ];
    let mut out: Vec<String> = Vec::new();
    for (name, f) in checks {
        if f(content) {
            out.push((*name).to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_url ----

    #[test]
    fn is_url_accepts_https() {
        assert!(is_url("https://example.com/path"));
        assert!(is_url("https://example.com"));
    }

    #[test]
    fn is_url_accepts_http_and_ftp() {
        assert!(is_url("http://example.com"));
        assert!(is_url("ftp://files.example.com/pub"));
        assert!(is_url("www.example.com/foo"));
    }

    #[test]
    fn is_url_rejects_plain_text() {
        assert!(!is_url("example.com"));
        assert!(!is_url("hello world"));
        assert!(!is_url(""));
        assert!(!is_url("   "));
    }

    #[test]
    fn is_url_trims_whitespace() {
        assert!(is_url("  https://example.com  "));
    }

    // ---- is_email ----

    #[test]
    fn is_email_accepts_typical_addresses() {
        assert!(is_email("user@example.com"));
        assert!(is_email("first.last+tag@sub.example.co.uk"));
    }

    #[test]
    fn is_email_rejects_invalid() {
        assert!(!is_email("user@"));
        assert!(!is_email("@example.com"));
        assert!(!is_email("user@example"));
        assert!(!is_email("plain string"));
        assert!(!is_email(""));
    }

    // ---- is_phone ----

    #[test]
    fn is_phone_accepts_chinese_mobile() {
        assert!(is_phone("13800138000"));
        assert!(is_phone("19912345678"));
        assert!(is_phone("15998765432"));
    }

    #[test]
    fn is_phone_rejects_non_chinese_mobile() {
        assert!(!is_phone("1234567890"));
        assert!(!is_phone("1380013800")); // 10 digits
        assert!(!is_phone("138001380000")); // 12 digits
        assert!(!is_phone("12800138000")); // 2nd digit 2 (not 3-9)
        assert!(!is_phone("hello"));
    }

    #[test]
    fn is_phone_accepts_e164() {
        assert!(is_phone("+8613800138000"));
        assert!(is_phone("+1 4155552671"));
        assert!(is_phone("+44-2071838750"));
    }

    // ---- is_idcard ----

    #[test]
    fn is_idcard_accepts_valid_18_digit() {
        assert!(is_idcard("11010519900307123X"));
        assert!(is_idcard("11010519900307123x"));
        assert!(is_idcard("110105199003071230"));
    }

    #[test]
    fn is_idcard_rejects_wrong_length() {
        assert!(!is_idcard("1101051990030712")); // 16
        assert!(!is_idcard("1101051990030712345")); // 19
        assert!(!is_idcard(""));
    }

    #[test]
    fn is_idcard_rejects_non_digit_tail() {
        assert!(!is_idcard("11010519900307123Y")); // not X/x
        assert!(!is_idcard("11010519900307123 ")); // space
    }

    // ---- is_ipv4 ----

    #[test]
    fn is_ipv4_accepts_valid_quads() {
        assert!(is_ipv4("0.0.0.0"));
        assert!(is_ipv4("192.168.1.1"));
        assert!(is_ipv4("255.255.255.255"));
        assert!(is_ipv4("127.0.0.1"));
    }

    #[test]
    fn is_ipv4_rejects_out_of_range_octets() {
        assert!(!is_ipv4("256.0.0.1"));
        assert!(!is_ipv4("999.999.999.999"));
        assert!(!is_ipv4("1.2.3.999"));
    }

    #[test]
    fn is_ipv4_rejects_wrong_shape() {
        assert!(!is_ipv4("1.2.3"));
        assert!(!is_ipv4("1.2.3.4.5"));
        assert!(!is_ipv4("1.2.3.a"));
        assert!(!is_ipv4(""));
    }

    // ---- is_ipv6 ----

    #[test]
    fn is_ipv6_accepts_full_form() {
        assert!(is_ipv6("2001:0db8:85a3:0000:0000:8a2e:0370:7334"));
        assert!(is_ipv6("fe80:0:0:0:202:b3ff:fe1e:8329"));
    }

    #[test]
    fn is_ipv6_accepts_compressed_form() {
        assert!(is_ipv6("::1"));
        assert!(is_ipv6("::"));
        assert!(is_ipv6("2001:db8::1"));
        assert!(is_ipv6("fe80::202:b3ff:fe1e:8329"));
    }

    #[test]
    fn is_ipv6_rejects_non_ipv6_strings() {
        assert!(!is_ipv6("1.2.3.4"));
        assert!(!is_ipv6("hello"));
        assert!(!is_ipv6(""));
        assert!(!is_ipv6("2001:db8:::1")); // triple colon
        assert!(!is_ipv6("gggg::1")); // non-hex
    }

    // ---- is_jwt ----

    #[test]
    fn is_jwt_accepts_three_segments() {
        // Real-shape JWT, each segment base64url, each ≥ 4 chars.
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert!(is_jwt(token));
    }

    #[test]
    fn is_jwt_rejects_wrong_segment_count() {
        assert!(!is_jwt("a.b")); // 2
        assert!(!is_jwt("a.b.c.d")); // 4
        assert!(!is_jwt("single"));
    }

    #[test]
    fn is_jwt_rejects_short_or_invalid_chars() {
        assert!(!is_jwt("a.b.c")); // segments < 4 chars
        assert!(!is_jwt("abc!.def.ghi")); // '!' not base64url
        assert!(!is_jwt(""));
    }

    // ---- is_path_windows ----

    #[test]
    fn is_path_windows_accepts_drive_letter() {
        assert!(is_path_windows("C:\\Users\\foo\\bar.txt"));
        assert!(is_path_windows("D:/projects/app/main.rs"));
        assert!(is_path_windows("c:\\"));
    }

    #[test]
    fn is_path_windows_accepts_unc() {
        assert!(is_path_windows("\\\\server\\share"));
        assert!(is_path_windows("\\\\server\\share\\folder\\file.txt"));
    }

    #[test]
    fn is_path_windows_rejects_unix_and_garbage() {
        assert!(!is_path_windows("/home/user"));
        assert!(!is_path_windows("user.txt"));
        assert!(!is_path_windows(""));
        assert!(!is_path_windows("C:bad")); // missing slash
    }

    // ---- is_path_unix ----

    #[test]
    fn is_path_unix_accepts_absolute_and_home() {
        assert!(is_path_unix("/home/user/doc.txt"));
        assert!(is_path_unix("/etc/nginx/nginx.conf"));
        assert!(is_path_unix("~/notes"));
        assert!(is_path_unix("~"));
    }

    #[test]
    fn is_path_unix_rejects_windows_and_bare_slash() {
        assert!(!is_path_unix("C:\\Users\\foo"));
        assert!(!is_path_unix("/"));
        assert!(!is_path_unix("relative/path"));
        assert!(!is_path_unix(""));
    }

    // ---- is_color_hex ----

    #[test]
    fn is_color_hex_accepts_valid_3_4_6_8() {
        assert!(is_color_hex("#fff"));
        assert!(is_color_hex("#ffff"));
        assert!(is_color_hex("#ffffff"));
        assert!(is_color_hex("#ffffffff"));
        assert!(is_color_hex("#AABBCC"));
    }

    #[test]
    fn is_color_hex_rejects_wrong_length_or_no_hash() {
        assert!(!is_color_hex("#ff"));
        assert!(!is_color_hex("#fffff"));
        assert!(!is_color_hex("#fffffff")); // 7
        assert!(!is_color_hex("#ggg"));
        assert!(!is_color_hex("ffffff"));
        assert!(!is_color_hex(""));
    }

    // ---- is_color_rgb ----

    #[test]
    fn is_color_rgb_accepts_valid() {
        assert!(is_color_rgb("rgb(255, 255, 255)"));
        assert!(is_color_rgb("rgb(0,0,0)"));
        assert!(is_color_rgb("rgba(0, 0, 0, 0.5)"));
        assert!(is_color_rgb("rgba(10, 20, 30, 100%)"));
    }

    #[test]
    fn is_color_rgb_rejects_out_of_range_or_malformed() {
        assert!(!is_color_rgb("rgb(256, 0, 0)"));
        assert!(!is_color_rgb("rgb(0, 0, 0, 0.5)")); // 4 args for rgb()
        assert!(!is_color_rgb("rgba(0, 0, 0, 2.0)")); // alpha > 1
        assert!(!is_color_rgb("rgba(0, 0, 0, 200%)")); // alpha > 100%
        assert!(!is_color_rgb("#ffffff"));
        assert!(!is_color_rgb(""));
    }

    // ---- is_json ----

    #[test]
    fn is_json_accepts_objects_and_arrays() {
        assert!(is_json(r#"{"a": 1}"#));
        assert!(is_json(r#"[1, 2, 3]"#));
        assert!(is_json(r#"  {"nested": {"x": [true, null, "s"]}}  "#));
    }

    #[test]
    fn is_json_rejects_malformed() {
        assert!(!is_json("{not json}"));
        assert!(!is_json("[1, 2,"));
        assert!(!is_json(""));
        assert!(!is_json("not even close {"));
    }

    #[test]
    fn is_json_rejects_non_json_braces() {
        // Leading-brace gate: prose containing braces must not be claimed.
        assert!(!is_json("hello { world }"));
    }

    // ---- classify (integration) ----

    #[test]
    fn classify_returns_empty_for_blank() {
        assert!(classify("").is_empty());
        assert!(classify("   ").is_empty());
    }

    #[test]
    fn classify_returns_one_kind_for_pure_url() {
        let kinds = classify("https://example.com/foo");
        assert_eq!(kinds, vec!["url".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_pure_email() {
        let kinds = classify("user@example.com");
        assert_eq!(kinds, vec!["email".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_color() {
        let kinds = classify("#fff");
        assert_eq!(kinds, vec!["color_hex".to_string()]);
        let kinds = classify("rgb(0, 0, 0)");
        assert_eq!(kinds, vec!["color_rgb".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_json() {
        let kinds = classify(r#"{"a": 1}"#);
        assert_eq!(kinds, vec!["json".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_phone() {
        let kinds = classify("13800138000");
        assert_eq!(kinds, vec!["phone".to_string()]);
    }

    #[test]
    fn classify_returns_idcard_for_idcard() {
        let kinds = classify("11010519900307123X");
        assert_eq!(kinds, vec!["idcard".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_ipv4() {
        let kinds = classify("192.168.1.1");
        assert_eq!(kinds, vec!["ipv4".to_string()]);
    }

    #[test]
    fn classify_returns_one_kind_for_path() {
        let kinds = classify("/home/user/note.txt");
        assert_eq!(kinds, vec!["path_unix".to_string()]);
        let kinds = classify("C:\\Users\\foo");
        assert_eq!(kinds, vec!["path_windows".to_string()]);
    }

    #[test]
    fn classify_returns_no_match_for_plain_text() {
        let kinds = classify("this is just a sentence with no special structure");
        assert!(kinds.is_empty(), "expected no match, got {kinds:?}");
    }

    #[test]
    fn classify_is_deterministic() {
        let input = "https://user@example.com has #fff and 192.168.1.1";
        // url wins first; email inside URL won't match the URL gate
        // (no scheme/host separation), so the rest cascade through.
        let a = classify(input);
        let b = classify(input);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn classify_handles_cjk_and_emoji_input() {
        // CJK / emoji content is not URL, email, or any other shape.
        let kinds = classify("你好，世界 🎉");
        assert!(kinds.is_empty(), "CJK / emoji should match nothing, got {kinds:?}");
    }

    #[test]
    fn classify_does_not_match_partial_keywords() {
        // "user@host" alone is not email (no TLD). "127.0.0" alone is not IPv4.
        let kinds = classify("user@host");
        assert!(!kinds.contains(&"email".to_string()), "{kinds:?}");
        let kinds = classify("127.0.0");
        assert!(!kinds.contains(&"ipv4".to_string()), "{kinds:?}");
    }

    #[test]
    fn content_kinds_is_in_dispatcher_order() {
        // If you reorder CONTENT_KINDS, integration tests above may need
        // to be re-checked for ordering assumptions. This test pins the
        // public contract.
        assert_eq!(CONTENT_KINDS.len(), 12);
        assert_eq!(CONTENT_KINDS[0], "url");
        assert_eq!(CONTENT_KINDS[1], "email");
        assert_eq!(CONTENT_KINDS[2], "phone");
        assert_eq!(CONTENT_KINDS[3], "idcard");
        assert_eq!(CONTENT_KINDS[4], "ipv4");
        assert_eq!(CONTENT_KINDS[5], "ipv6");
        assert_eq!(CONTENT_KINDS[6], "jwt");
        assert_eq!(CONTENT_KINDS[7], "path_windows");
        assert_eq!(CONTENT_KINDS[8], "path_unix");
        assert_eq!(CONTENT_KINDS[9], "color_hex");
        assert_eq!(CONTENT_KINDS[10], "color_rgb");
        assert_eq!(CONTENT_KINDS[11], "json");
    }
}
