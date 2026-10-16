//! Layout labels from the same native xkbcommon keymaps as the compositor.
use serde_json::{json, Map, Value};
use std::{collections::HashSet, fs};
use unicode_general_category::{get_general_category, GeneralCategory as Category};
use unicode_normalization::char::canonical_combining_class;
use xkbcommon::xkb;

fn dead_key(name: &str) -> (&'static str, &'static str) {
    match name {
        "grave" => ("`", "\u{300}"),
        "acute" => ("´", "\u{301}"),
        "circumflex" => ("^", "\u{302}"),
        "tilde" => ("~", "\u{303}"),
        "macron" => ("¯", "\u{304}"),
        "breve" => ("˘", "\u{306}"),
        "abovedot" => ("˙", "\u{307}"),
        "diaeresis" => ("¨", "\u{308}"),
        "abovering" => ("˚", "\u{30a}"),
        "doubleacute" => ("˝", "\u{30b}"),
        "caron" => ("ˇ", "\u{30c}"),
        "cedilla" => ("¸", "\u{327}"),
        "ogonek" => ("˛", "\u{328}"),
        "iota" => ("ͺ", ""),
        "belowdot" => (".", "\u{323}"),
        "hook" => ("̉", "\u{309}"),
        "horn" => ("̛", "\u{31b}"),
        "stroke" => ("/", ""),
        "greek" => ("α", ""),
        "currency" => ("¤", ""),
        _ => ("", ""),
    }
}

fn label(state: &xkb::State, code: u32) -> (String, &'static str) {
    let code = xkb::Keycode::new(code + 8);
    let sym = xkb::keysym_get_name(state.key_get_one_sym(code));
    let dead = dead_key(sym.strip_prefix("dead_").unwrap_or(""));
    let mut text = state.key_get_utf8(code);
    if text.is_empty() {
        return (dead.0.into(), dead.1);
    }
    // Python str.isprintable excludes every Other/Separator category except
    // ASCII space; isspace then rejects whitespace-only strings.
    if text.chars().any(|c| {
        matches!(
            get_general_category(c),
            Category::Control
                | Category::Format
                | Category::Surrogate
                | Category::PrivateUse
                | Category::Unassigned
                | Category::LineSeparator
                | Category::ParagraphSeparator
                | Category::SpaceSeparator
        ) && c != ' '
    }) || text.chars().all(char::is_whitespace)
    {
        return (String::new(), dead.1);
    }
    if text.chars().all(|c| canonical_combining_class(c) != 0) {
        text.insert(0, '◌');
    }
    (text, dead.1)
}

pub fn labels(description: &str) -> Result<Value, String> {
    if description.contains('\0') {
        return Err("Invalid layout description".into());
    }
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let mut resolved = None;
    'paths: for root in context.include_paths() {
        let Ok(text) = fs::read_to_string(root.join("rules/evdev.lst")) else {
            continue;
        };
        let mut section = "";
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("! ") {
                section = name.trim();
                continue;
            }
            let Some((name, desc)) = line.trim().split_once(char::is_whitespace) else {
                continue;
            };
            let desc = desc.trim();
            if section == "layout" && desc == description {
                resolved = Some((name.to_owned(), String::new()));
                break 'paths;
            }
            if section == "variant" {
                if let Some((layout, value)) = desc.split_once(':') {
                    if value.trim() == description {
                        resolved = Some((layout.trim().to_owned(), name.to_owned()));
                        break 'paths;
                    }
                }
            }
        }
    }
    let Some((layout, variant)) = resolved else {
        return Ok(json!({"error": format!("unknown layout: {description}")}));
    };
    let Some(keymap) = xkb::Keymap::new_from_names(
        &context,
        "evdev",
        "pc105",
        &layout,
        &variant,
        Some("".into()),
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ) else {
        return Ok(json!({"error":format!("xkbcommon could not compile {layout}({variant})")}));
    };
    let plain = xkb::State::new(&keymap);
    let mut shifted = xkb::State::new(&keymap);
    let shift = keymap.mod_get_index("Shift");
    if shift >= 32 {
        return Err("The keymap has no Shift modifier".into());
    }
    shifted.update_mask(1 << shift, 0, 0, 0, 0, 0);
    let mut keys = Map::new();
    let mut combine = Map::new();
    let mut letters = Vec::new();
    for code in (2..14)
        .chain(16..28)
        .chain(30..42)
        .chain(43..54)
        .chain([86])
    {
        let (base, base_mark) = label(&plain, code);
        let (shifted, shifted_mark) = label(&shifted, code);
        if base.chars().find(|c| *c != '◌').is_some_and(|c| {
            matches!(
                get_general_category(c),
                Category::UppercaseLetter
                    | Category::LowercaseLetter
                    | Category::TitlecaseLetter
                    | Category::ModifierLetter
                    | Category::OtherLetter
                    | Category::NonspacingMark
                    | Category::SpacingMark
                    | Category::EnclosingMark
            )
        }) {
            letters.push(code);
        }
        if !base_mark.is_empty() || !shifted_mark.is_empty() {
            combine.insert(code.to_string(), json!([base_mark, shifted_mark]));
        }
        keys.insert(code.to_string(), json!([base, shifted]));
    }
    let elsewhere: HashSet<_> = keys
        .iter()
        .filter(|(key, _)| *key != "86")
        .flat_map(|(_, pair)| pair.as_array().unwrap().iter().filter_map(Value::as_str))
        .collect();
    let iso = keys["86"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .any(|v| !v.is_empty() && !elsewhere.contains(v));
    Ok(
        json!({"layout": layout, "variant": variant, "iso": iso, "keys": keys, "letters": letters, "combine": combine}),
    )
}
