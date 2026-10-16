//! Clipboard-history cleanup, deliberately not a generic HTML sanitizer.
use regex::Regex;
use std::sync::LazyLock;

fn strip_active_tags(input: &str) -> String {
    static OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<(script|style)\b").unwrap());
    static SCRIPT_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</script>").unwrap());
    static STYLE_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</style>").unwrap());
    let mut output = String::new();
    let mut cursor = 0;
    // Match the original combined expression's left-to-right, non-overlapping
    // behavior even for malformed nested script/style markup.
    for capture in OPEN.captures_iter(input) {
        let open = capture.get(0).unwrap();
        if open.start() < cursor {
            continue;
        }
        let close = if capture[1].eq_ignore_ascii_case("script") {
            &*SCRIPT_END
        } else {
            &*STYLE_END
        };
        if let Some(end) = close.find(&input[open.end()..]) {
            output.push_str(&input[cursor..open.start()]);
            cursor = open.end() + end.end();
        }
    }
    output.push_str(&input[cursor..]);
    output
}

// Python's html.unescape follows HTML5, including semicolon-less legacy names,
// Windows-1252 numeric references and invalid-codepoint handling. A generic
// entity decoder alone is not a compatible replacement for clipboard history.
pub fn unescape(input: &str) -> String {
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0c <&#;]{1,32};?)").unwrap()
    });
    const C1: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    let named = |name: &str| -> Option<String> {
        let &(first, second) = web_atoms::NAMED_ENTITIES.get(name)?;
        // The tokenizer table also contains zero-valued prefix sentinels.
        if first == 0 {
            return None;
        }
        let mut value = char::from_u32(first)?.to_string();
        if second != 0 {
            value.push(char::from_u32(second)?);
        }
        Some(value)
    };
    ENTITY
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let entity = &caps[1];
            if let Some(number) = entity.strip_prefix('#') {
                let number = number.trim_end_matches(';');
                let value = if number.starts_with(['x', 'X']) {
                    u64::from_str_radix(&number[1..], 16)
                } else {
                    number.parse::<u64>()
                };
                let value = value.unwrap_or(u64::MAX);
                return match value {
                    0 | 0xd800..=0xdfff | 0x110000.. => "\u{fffd}".into(),
                    0x80..=0x9f => C1[(value - 0x80) as usize].to_string(),
                    1..=8 | 11 | 14..=31 | 127 | 0xfdd0..=0xfdef => String::new(),
                    value if value & 0xffff >= 0xfffe => String::new(),
                    value => char::from_u32(value as u32).unwrap().to_string(),
                };
            }
            if let Some(value) = named(entity) {
                return value;
            }
            for (end, _) in entity.char_indices().rev().filter(|(end, _)| *end > 1) {
                if let Some(value) = named(&entity[..end]) {
                    return format!("{value}{}", &entity[end..]);
                }
            }
            format!("&{entity}")
        })
        .into_owned()
}

pub fn is_browser_markup(payload: &[u8]) -> bool {
    payload.starts_with(b"<meta http-equiv=\"content-type\" content=\"text/html")
        || payload
            .windows(b"<!--StartFragment-->".len())
            .any(|w| w == b"<!--StartFragment-->")
}

/// None means an empty markup-only entry should not be added to history.
/// Non-browser payloads are copied byte-for-byte, including images and newlines.
pub fn filter(payload: &[u8]) -> Option<Vec<u8>> {
    if !is_browser_markup(payload) {
        return Some(payload.to_vec());
    }
    static META: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<meta[^>]*>").unwrap());
    static COMMENTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
    static BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static BLOCKS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)</(p|div|li|tr|h[1-6]|blockquote|pre)>").unwrap());
    static IMAGES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)<img[^>]*\bsrc=["']([^"']+)["'][^>]*>"#).unwrap());
    static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
    static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+\n").unwrap());
    static LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
    let mut text = META
        .replace(&String::from_utf8_lossy(payload), "")
        .into_owned();
    text = strip_active_tags(&text);
    text = COMMENTS.replace_all(&text, "").into_owned();
    text = BREAKS.replace_all(&text, "\n").into_owned();
    text = BLOCKS.replace_all(&text, "\n").into_owned();
    text = IMAGES.replace_all(&text, "$1").into_owned();
    text = TAGS.replace_all(&text, "").into_owned();
    text = unescape(&text);
    text = SPACES.replace_all(&text, "\n").into_owned();
    text = LINES
        .replace_all(&text, "\n\n")
        .trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
        .to_owned();
    (!text.is_empty()).then(|| text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_payloads_are_not_html() {
        for data in [
            b"<div>source code</div>\n".as_slice(),
            b"\x89PNG\r\n\x1a\n\0\xff",
            b"exact\r\n\n",
            b"",
        ] {
            assert_eq!(filter(data).unwrap(), data);
        }
    }
    #[test]
    fn browser_markup_preserves_text_structure_and_image_source() {
        assert_eq!(filter(b"<meta http-equiv=\"content-type\" content=\"text/html\"><p>Hello <b>world</b> &amp; friends</p><div>next<br>line</div>").unwrap(), b"Hello world & friends\nnext\nline");
        assert_eq!(filter(b"<html><!--StartFragment--><script>bad()</script><img src='https://example.test/image.png'><!--EndFragment--></html>").unwrap(), b"https://example.test/image.png");
        assert_eq!(filter(b"<!--StartFragment--><style>body{}</style>"), None);
    }
    #[test]
    fn html5_entity_edge_cases_match_the_existing_filter() {
        assert_eq!(unescape("&notit; &amp missing &notin; &#x80; &#0; &#xD800; &#1; &#999999999999999999999999;"), "¬it; & missing ∉ € � �  �");
        assert_eq!(
            unescape("&NotEqualTilde; &unknown; &#xFFFF;"),
            "≂\u{338} &unknown; "
        );
    }
}
