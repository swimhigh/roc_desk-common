//! Text-file encoding detection/conversion shared by every tool that reads or
//! writes local files (ported from `roc_desk` host `fsops::encoding`).
//!
//! Windows-authored `.txt`/`.log`/`.ini`/`.h` files are very often GBK, not
//! UTF-8, so a naive `String::from_utf8` either fails outright (local) or
//! silently mangles the text via `from_utf8_lossy` (remote). Detection order:
//! BOM first (explicit) -> strict UTF-8 -> GBK -> UTF-8 lossy fallback, so any
//! byte sequence still returns *something* readable.

/// Encoding labels offered by the "Reopen/Save with Encoding" UI (mirrors VS Code).
pub const SUPPORTED_ENCODINGS: &[&str] = &["UTF-8", "GBK", "UTF-16LE", "UTF-16BE"];

pub fn decode_text_detect(bytes: &[u8]) -> (String, &'static str) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), "UTF-8");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (
            encoding_rs::UTF_16LE.decode(rest).0.into_owned(),
            "UTF-16LE",
        );
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (
            encoding_rs::UTF_16BE.decode(rest).0.into_owned(),
            "UTF-16BE",
        );
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), "UTF-8");
    }
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if !had_errors {
        return (text.into_owned(), "GBK");
    }
    (String::from_utf8_lossy(bytes).into_owned(), "UTF-8")
}

pub fn decode_text(bytes: &[u8]) -> String {
    decode_text_detect(bytes).0
}

fn encoding_for(label: &str) -> Option<&'static encoding_rs::Encoding> {
    match label {
        "UTF-8" => Some(encoding_rs::UTF_8),
        "GBK" => Some(encoding_rs::GBK),
        "UTF-16LE" => Some(encoding_rs::UTF_16LE),
        "UTF-16BE" => Some(encoding_rs::UTF_16BE),
        _ => None,
    }
}

/// Explicit "Reopen with Encoding": skip auto-detection, decode with the given
/// label (stripping any matching BOM so it isn't decoded as a stray character).
pub fn decode_with(bytes: &[u8], label: &str) -> Result<String, String> {
    let enc = encoding_for(label).ok_or_else(|| format!("Unsupported encoding: {label}"))?;
    let bytes = match label {
        "UTF-8" => bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes),
        "UTF-16LE" => bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes),
        "UTF-16BE" => bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes),
        _ => bytes,
    };
    Ok(enc.decode(bytes).0.into_owned())
}

/// "Save with Encoding": encode text back to bytes with the given label before writing.
pub fn encode_with(text: &str, label: &str) -> Result<Vec<u8>, String> {
    let enc = encoding_for(label).ok_or_else(|| format!("Unsupported encoding: {label}"))?;
    Ok(enc.encode(text).0.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_utf8() {
        assert_eq!(decode_text("你好 hello".as_bytes()), "你好 hello");
    }

    #[test]
    fn decodes_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello".as_bytes());
        assert_eq!(decode_text(&bytes), "hello");
    }

    #[test]
    fn decodes_gbk() {
        let (encoded, _, _) = encoding_rs::GBK.encode("你好世界");
        assert_eq!(decode_text(&encoded), "你好世界");
    }

    #[test]
    fn roundtrips_forced_encoding() {
        let bytes = encode_with("你好世界", "GBK").unwrap();
        assert_eq!(decode_with(&bytes, "GBK").unwrap(), "你好世界");
    }
}
