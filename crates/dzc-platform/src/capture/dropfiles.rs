const HEADER_BYTES: usize = 20;

pub(crate) fn parse_dropfiles(raw: &[u8]) -> Option<Vec<String>> {
    if raw.len() < HEADER_BYTES {
        return None;
    }
    let offset = u32::from_le_bytes(raw[..4].try_into().ok()?) as usize;
    if offset < HEADER_BYTES {
        return None;
    }
    let data = raw.get(offset..)?;
    let wide = raw[16..20].iter().any(|byte| *byte != 0);
    let mut files = Vec::new();
    if wide {
        let mut path = Vec::new();
        for pair in data.chunks_exact(2) {
            let unit = u16::from_le_bytes([pair[0], pair[1]]);
            if unit == 0 {
                if path.is_empty() {
                    return (!files.is_empty()).then_some(files);
                }
                files.push(String::from_utf16(&path).ok()?);
                path.clear();
            } else {
                path.push(unit);
            }
        }
    } else {
        let mut remaining = data;
        loop {
            let end = remaining.iter().position(|byte| *byte == 0)?;
            if end == 0 {
                return (!files.is_empty()).then_some(files);
            }
            files.push(std::str::from_utf8(&remaining[..end]).ok()?.to_string());
            remaining = &remaining[end + 1..];
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(wide: bool, offset: usize, bytes: &[u8]) -> Vec<u8> {
        let mut raw = vec![0; offset];
        raw[..4].copy_from_slice(&(offset as u32).to_le_bytes());
        raw[16..20].copy_from_slice(&u32::from(wide).to_le_bytes());
        raw.extend_from_slice(bytes);
        raw
    }

    fn unicode(paths: &[&str]) -> Vec<u8> {
        let mut units = Vec::new();
        for path in paths {
            units.extend(path.encode_utf16());
            units.push(0);
        }
        units.push(0);
        units.into_iter().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_multiple_unicode_paths() {
        let paths = ["C:\\文件\\一.png", "D:\\two.txt"];
        assert_eq!(parse_dropfiles(&payload(true, 20, &unicode(&paths))),
            Some(paths.map(str::to_string).to_vec()));
    }

    #[test]
    fn byte_offset_need_not_be_aligned_for_unicode() {
        assert_eq!(parse_dropfiles(&payload(true, 21, &unicode(&["C:\\one"]))),
            Some(vec!["C:\\one".to_string()]));
    }

    #[test]
    fn preserves_existing_utf8_ansi_fallback() {
        assert_eq!(parse_dropfiles(&payload(false, 20, b"C:\\one\0D:\\two\0\0")),
            Some(vec!["C:\\one".to_string(), "D:\\two".to_string()]));
    }

    #[test]
    fn rejects_truncated_headers_and_offsets_outside_file_list() {
        for length in 0..20 {
            assert_eq!(parse_dropfiles(&vec![0; length]), None);
        }
        for offset in [0, 1, 19, 25, u32::MAX] {
            let mut raw = payload(false, 20, b"x\0\0");
            raw[..4].copy_from_slice(&offset.to_le_bytes());
            assert_eq!(parse_dropfiles(&raw), None);
        }
    }

    #[test]
    fn rejects_unterminated_or_singly_terminated_file_lists() {
        for data in [b"one".as_slice(), b"one\0".as_slice()] {
            assert_eq!(parse_dropfiles(&payload(false, 20, data)), None);
        }
        let mut data = unicode(&["one"]);
        data.truncate(data.len() - 2);
        assert_eq!(parse_dropfiles(&payload(true, 20, &data)), None);
        data.pop();
        assert_eq!(parse_dropfiles(&payload(true, 20, &data)), None);
    }

    #[test]
    fn rejects_a_partial_final_unicode_terminator() {
        let mut data = unicode(&["one"]);
        data.pop();
        assert_eq!(parse_dropfiles(&payload(true, 20, &data)), None);
    }

    #[test]
    fn rejects_invalid_encoding_without_returning_partial_paths() {
        let invalid_wide: Vec<u8> = [0xd800u16, 0, 0].into_iter().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse_dropfiles(&payload(true, 20, &invalid_wide)), None);
        assert_eq!(parse_dropfiles(&payload(false, 20, b"good\0\xff\0\0")), None);
    }

    #[test]
    fn empty_lists_and_offsets_at_the_end_return_none() {
        assert_eq!(parse_dropfiles(&payload(false, 20, &[0, 0])), None);
        assert_eq!(parse_dropfiles(&payload(true, 20, &[0, 0, 0, 0])), None);
        assert_eq!(parse_dropfiles(&payload(true, 20, &[])), None);
    }

    #[test]
    fn ignores_allocation_padding_after_complete_termination() {
        let mut data = unicode(&["one"]);
        data.extend_from_slice(&[0xff, 0xfe, 0xff]);
        assert_eq!(parse_dropfiles(&payload(true, 20, &data)), Some(vec!["one".to_string()]));
    }
}
