//! Shareable JSON import/export and schema validation for custom conversion recipes.
//!
//! Pure std JSON parser/serializer so `crates/core` stays dependency-free and offline.

use crate::ops;
use crate::{Error, Result};
use std::collections::HashMap;

pub const RECIPE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeSpec {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub steps: String,
}

pub fn export_recipes_json(recipes: &[RecipeSpec]) -> String {
    let mut out = String::from("{\n  \"version\": 1,\n  \"recipes\": [\n");
    for (i, r) in recipes.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"version\": {},\n",
            if r.version == 0 { RECIPE_SCHEMA_VERSION } else { r.version }
        ));
        out.push_str(&format!("      \"name\": {},\n", json_quote(&r.name)));
        out.push_str(&format!("      \"description\": {},\n", json_quote(&r.description)));
        out.push_str(&format!("      \"steps\": {}\n", json_quote(&r.steps)));
        out.push_str(if i + 1 < recipes.len() { "    },\n" } else { "    }\n" });
    }
    out.push_str("  ]\n}\n");
    out
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, PartialEq)]
enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(HashMap<String, JsonVal>),
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { bytes: input.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn parse(&mut self) -> Result<JsonVal> {
        let v = self.value()?;
        self.skip_ws();
        if self.pos != self.bytes.len() {
            return Err(Error::BadOption("trailing characters in recipe JSON".into()));
        }
        Ok(v)
    }

    fn value(&mut self) -> Result<JsonVal> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(JsonVal::Str(self.string()?)),
            Some(b't') => self.lit(b"true", JsonVal::Bool(true)),
            Some(b'f') => self.lit(b"false", JsonVal::Bool(false)),
            Some(b'n') => self.lit(b"null", JsonVal::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(Error::BadOption("invalid JSON syntax in recipe".into())),
        }
    }

    fn lit(&mut self, expected: &[u8], val: JsonVal) -> Result<JsonVal> {
        if self.bytes.get(self.pos..self.pos + expected.len()) == Some(expected) {
            self.pos += expected.len();
            Ok(val)
        } else {
            Err(Error::BadOption("invalid JSON literal".into()))
        }
    }

    fn number(&mut self) -> Result<JsonVal> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| Error::BadOption("bad number".into()))?;
        let n = s.parse::<f64>().map_err(|_| Error::BadOption(format!("bad number {s}")))?;
        Ok(JsonVal::Num(n))
    }

    fn string(&mut self) -> Result<String> {
        if self.next() != Some(b'"') {
            return Err(Error::BadOption("expected string".into()));
        }
        let mut out = String::new();
        while let Some(b) = self.next() {
            match b {
                b'"' => return Ok(out),
                b'\\' => match self.next() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'b') => out.push('\x08'),
                    Some(b'f') => out.push('\x0c'),
                    Some(b'u') => {
                        let hex = self
                            .bytes
                            .get(self.pos..self.pos + 4)
                            .and_then(|s| std::str::from_utf8(s).ok())
                            .and_then(|s| u32::from_str_radix(s, 16).ok())
                            .and_then(char::from_u32)
                            .ok_or_else(|| Error::BadOption("invalid \\u escape".into()))?;
                        self.pos += 4;
                        out.push(hex);
                    }
                    _ => return Err(Error::BadOption("invalid escape in recipe JSON".into())),
                },
                other if other < 0x80 => out.push(other as char),
                first => {
                    // Decode multi-byte UTF-8 sequence starting at `first`.
                    let start = self.pos - 1;
                    let width = if first & 0b1110_0000 == 0b1100_0000 {
                        2
                    } else if first & 0b1111_0000 == 0b1110_0000 {
                        3
                    } else if first & 0b1111_1000 == 0b1111_0000 {
                        4
                    } else {
                        return Err(Error::BadOption("invalid UTF-8 in recipe JSON".into()));
                    };
                    let end = start + width;
                    let slice = self.bytes.get(start..end).ok_or_else(|| Error::BadOption("truncated UTF-8".into()))?;
                    let ch = std::str::from_utf8(slice).map_err(|_| Error::BadOption("invalid UTF-8".into()))?;
                    out.push_str(ch);
                    self.pos = end;
                }
            }
        }
        Err(Error::BadOption("unterminated string in recipe JSON".into()))
    }

    fn array(&mut self) -> Result<JsonVal> {
        self.next();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.next();
            return Ok(JsonVal::Arr(items));
        }
        loop {
            items.push(self.value()?);
            match self.peek() {
                Some(b',') => {
                    self.next();
                }
                Some(b']') => {
                    self.next();
                    return Ok(JsonVal::Arr(items));
                }
                _ => return Err(Error::BadOption("expected ',' or ']' in array".into())),
            }
        }
    }

    fn object(&mut self) -> Result<JsonVal> {
        self.next();
        let mut map = HashMap::new();
        if self.peek() == Some(b'}') {
            self.next();
            return Ok(JsonVal::Obj(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            if self.peek() != Some(b':') {
                return Err(Error::BadOption("expected ':' in object".into()));
            }
            self.next();
            let val = self.value()?;
            map.insert(key, val);
            match self.peek() {
                Some(b',') => {
                    self.next();
                }
                Some(b'}') => {
                    self.next();
                    return Ok(JsonVal::Obj(map));
                }
                _ => return Err(Error::BadOption("expected ',' or '}' in object".into())),
            }
        }
    }
}

fn validate_recipe_obj(obj: &HashMap<String, JsonVal>, default_version: u32) -> Result<RecipeSpec> {
    let version = match obj.get("version") {
        Some(JsonVal::Num(n)) => {
            if *n != 1.0 {
                return Err(Error::BadOption(format!("unsupported recipe schema version {n}")));
            }
            1
        }
        Some(_) => return Err(Error::BadOption("recipe version must be a number".into())),
        None => default_version,
    };
    if version != RECIPE_SCHEMA_VERSION {
        return Err(Error::BadOption(format!("unsupported recipe schema version {version}")));
    }
    let name = match obj.get("name") {
        Some(JsonVal::Str(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err(Error::BadOption("recipe is missing a non-empty `name`".into())),
    };
    let description = match obj.get("description") {
        Some(JsonVal::Str(s)) => s.trim().to_string(),
        Some(JsonVal::Null) | None => String::new(),
        _ => return Err(Error::BadOption("recipe `description` must be a string".into())),
    };
    let steps = match obj.get("steps") {
        Some(JsonVal::Str(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err(Error::BadOption(format!("recipe `{name}` is missing `steps`"))),
    };
    ops::parse_recipe(&steps).map_err(|e| Error::BadOption(format!("{name}: {e}")))?;
    Ok(RecipeSpec { version, name, description, steps })
}

/// Parses and validates a shareable recipe JSON payload (either `{"version":1,"recipes":[...]}`
/// or a single recipe object `{"version":1,"name":...,"steps":...}` or an array of recipes).
pub fn parse_and_validate_recipe_json(json: &str) -> Result<Vec<RecipeSpec>> {
    let parsed = Parser::new(json).parse()?;
    match parsed {
        JsonVal::Obj(ref map) => {
            let root_version = match map.get("version") {
                Some(JsonVal::Num(n)) => {
                    if *n != 1.0 {
                        return Err(Error::BadOption(format!("unsupported recipe schema version {n}")));
                    }
                    1
                }
                Some(_) => return Err(Error::BadOption("`version` must be a number".into())),
                None => RECIPE_SCHEMA_VERSION,
            };
            if let Some(list) = map.get("recipes") {
                let JsonVal::Arr(items) = list else {
                    return Err(Error::BadOption("`recipes` must be an array".into()));
                };
                if items.is_empty() {
                    return Err(Error::BadOption("`recipes` array is empty".into()));
                }
                items
                    .iter()
                    .map(|item| match item {
                        JsonVal::Obj(o) => validate_recipe_obj(o, root_version),
                        _ => Err(Error::BadOption("every item in `recipes` must be an object".into())),
                    })
                    .collect()
            } else {
                Ok(vec![validate_recipe_obj(map, root_version)?])
            }
        }
        JsonVal::Arr(items) => {
            if items.is_empty() {
                return Err(Error::BadOption("recipe list is empty".into()));
            }
            items
                .iter()
                .map(|item| match item {
                    JsonVal::Obj(o) => validate_recipe_obj(o, RECIPE_SCHEMA_VERSION),
                    _ => Err(Error::BadOption("every recipe must be an object".into())),
                })
                .collect()
        }
        _ => Err(Error::BadOption("expected a recipe JSON object or array".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_json_roundtrip_and_utf8() {
        let recipes = vec![
            RecipeSpec {
                version: 1,
                name: "WhatsApp Video · ضغط واتساب".into(),
                description: "Compress and strip metadata".into(),
                steps: "compress percent=60 | strip".into(),
            },
            RecipeSpec {
                version: 1,
                name: "Web Thumb".into(),
                description: String::new(),
                steps: "resize max=1200 | strip".into(),
            },
        ];
        let json = export_recipes_json(&recipes);
        let imported = parse_and_validate_recipe_json(&json).unwrap();
        assert_eq!(imported, recipes);
    }

    #[test]
    fn recipe_json_rejects_bad_schema_or_steps() {
        assert!(parse_and_validate_recipe_json(r#"{"version": 2, "recipes": []}"#).is_err());
        assert!(parse_and_validate_recipe_json(r#"{"version": 1, "recipes": []}"#).is_err());
        assert!(parse_and_validate_recipe_json(r#"{"version": 1, "name": "", "steps": "strip"}"#).is_err());
        assert!(parse_and_validate_recipe_json(r#"{"version": 1, "name": "Bad", "steps": "bogus_op"}"#).is_err());
        assert!(parse_and_validate_recipe_json(r#"{"version": 1, "name": "Single", "steps": "strip"}"#).is_err());
        assert!(parse_and_validate_recipe_json(r#"{"version": 1, "name": "One", "steps": "resize max=1000 | strip"}"#)
            .is_ok());
    }
}
