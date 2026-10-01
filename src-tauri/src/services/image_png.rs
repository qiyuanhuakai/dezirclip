//! Short-circuit for images that are already PNG.
//!
//! Three clipboard paths — pasting an image, the Windows clipboard fallback, and
//! copying an image back out — each used to decode the source image and encode
//! the result as PNG before handing it to base64. For a source that already *is*
//! a PNG that round trip reproduces the same pixels at the cost of a full decode
//! (a 4K screenshot is ~33 MB of RGBA) plus a full re-encode, on the hot path of
//! every image paste. `as_png_bytes` returns the original bytes untouched in that
//! case and only decodes for formats that genuinely need converting.

/// ISO/IEC 15948 signature: every valid PNG stream starts with these 8 bytes.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// True when `bytes` begins with the PNG signature.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > PNG_SIGNATURE.len() && bytes.starts_with(&PNG_SIGNATURE)
}

/// Returns `source` as PNG bytes, decoding and re-encoding only when the source
/// is in some other format. The error is whatever the `image` crate reports for
/// an undecodable source, so callers keep their existing failure behaviour.
pub fn as_png_bytes(source: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    if is_png(source) {
        return Ok(source.to_vec());
    }
    let decoded = image::load_from_memory(source)?;
    let mut encoded = Vec::new();
    decoded.write_to(
        &mut std::io::Cursor::new(&mut encoded),
        image::ImageFormat::Png,
    )?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_signature() {
        let mut with_payload = PNG_SIGNATURE.to_vec();
        with_payload.extend_from_slice(b"trailing");
        assert!(is_png(&with_payload));
    }

    #[test]
    fn rejects_other_magic_and_truncated_signature() {
        assert!(!is_png(b"GIF89a"));
        assert!(!is_png(b"\x89PNG\r\n\x1a"));
        assert!(!is_png(b""));
        // A bare signature with no image data behind it is not something we can
        // hand to a consumer, so it falls through to the decoder and fails there.
        assert!(!is_png(&PNG_SIGNATURE));
    }

    #[test]
    fn png_input_is_returned_verbatim() {
        let mut source = PNG_SIGNATURE.to_vec();
        source.extend_from_slice(b"not really a decodable png body");
        // A real decode would fail here, so reaching Ok proves the short circuit.
        assert_eq!(as_png_bytes(&source).unwrap(), source);
    }

    #[test]
    fn non_png_input_is_reencoded() {
        // 1x1 GIF header plus a real GIF body.
        let gif: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\
\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00\x3b";
        let png = as_png_bytes(&gif).expect("gif should decode and re-encode");
        assert!(is_png(&png));
    }

    #[test]
    fn garbage_input_reports_decode_error() {
        assert!(as_png_bytes(b"definitely not an image").is_err());
    }
}
