//! Small bounded JSON codec for the offline, dependency-free stdio transport.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}
impl Json {
    pub fn object(items: impl IntoIterator<Item = (impl Into<String>, Json)>) -> Self {
        Self::Object(items.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self { Self::Object(v) => v.get(key), _ => None }
    }
    pub fn text(&self) -> Option<&str> {
        match self { Self::String(v) => Some(v), _ => None }
    }
    pub fn fields(&self) -> Option<&BTreeMap<String, Self>> {
        match self { Self::Object(v) => Some(v), _ => None }
    }
    pub fn integer(&self) -> Option<i64> {
        match self {
            Self::Number(v) => {
                // Exact decimal arithmetic: rounding through f64 could silently
                // accept a fractional coordinate or alter an integer request ID.
                let (mantissa, exponent) = v.find(['e', 'E']).map_or((v.as_str(), "0"), |at| (&v[..at], &v[at + 1..]));
                let negative = mantissa.starts_with('-');
                let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
                let digits = digits.trim_start_matches('0');
                if digits.is_empty() { return Some(0); }
                let fraction = mantissa.split_once('.').map_or(0, |(_, fraction)| fraction.len() as i64);
                let shift = exponent.parse::<i64>().ok()?.checked_sub(fraction)?;
                let magnitude = if shift >= 0 {
                    if shift > 19 || digits.len() as i64 + shift > 19 { return None; }
                    format!("{digits}{}", "0".repeat(shift as usize))
                } else {
                    let cut = usize::try_from(shift.checked_neg()?).ok()?;
                    if cut >= digits.len() || !digits.as_bytes()[digits.len() - cut..].iter().all(|v| *v == b'0') { return None; }
                    digits[..digits.len() - cut].into()
                };
                format!("{}{magnitude}", if negative { "-" } else { "" }).parse().ok()
            }
            _ => None,
        }
    }
    pub fn encode(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(v) => v.to_string(),
            Self::Number(v) => v.clone(),
            Self::String(v) => {
                let mut out = String::from("\"");
                for ch in v.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            Self::Array(v) => format!("[{}]", v.iter().map(Self::encode).collect::<Vec<_>>().join(",")),
            Self::Object(v) => format!("{{{}}}", v.iter().map(|(k, v)|
                format!("{}:{}", Self::String(k.clone()).encode(), v.encode())).collect::<Vec<_>>().join(",")),
        }
    }
}
impl From<&str> for Json { fn from(v: &str) -> Self { Self::String(v.into()) } }
impl From<String> for Json { fn from(v: String) -> Self { Self::String(v) } }
impl From<bool> for Json { fn from(v: bool) -> Self { Self::Bool(v) } }
impl From<u32> for Json { fn from(v: u32) -> Self { Self::Number(v.to_string()) } }
impl From<i64> for Json { fn from(v: i64) -> Self { Self::Number(v.to_string()) } }

pub(crate) fn parse(text: &str) -> Result<Json, String> {
    struct Parser<'a> { text: &'a str, at: usize }
    impl Parser<'_> {
        fn byte(&self) -> Option<u8> { self.text.as_bytes().get(self.at).copied() }
        fn whitespace(&mut self) { while matches!(self.byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) { self.at += 1; } }
        fn error(&self) -> String { format!("invalid JSON at byte {}", self.at) }
        fn expect(&mut self, byte: u8) -> Result<(), String> {
            if self.byte() != Some(byte) { return Err(self.error()); }
            self.at += 1; Ok(())
        }
        fn hex(&mut self) -> Result<u16, String> {
            let mut result = 0;
            for _ in 0..4 {
                let n = self.byte().and_then(|v| (v as char).to_digit(16)).ok_or_else(|| self.error())?;
                result = result * 16 + n as u16; self.at += 1;
            }
            Ok(result)
        }
        fn string(&mut self) -> Result<String, String> {
            self.expect(b'"')?;
            let mut out = String::new();
            loop {
                match self.byte().ok_or_else(|| self.error())? {
                    b'"' => { self.at += 1; return Ok(out); }
                    b'\\' => {
                        self.at += 1;
                        let byte = self.byte().ok_or_else(|| self.error())?;
                        self.at += 1;
                        out.push(match byte {
                            b'"' => '"', b'\\' => '\\', b'/' => '/', b'b' => '\u{8}',
                            b'f' => '\u{c}', b'n' => '\n', b'r' => '\r', b't' => '\t',
                            b'u' => {
                                let first = self.hex()?;
                                let scalar = if (0xd800..=0xdbff).contains(&first) {
                                    self.expect(b'\\')?; self.expect(b'u')?;
                                    let second = self.hex()?;
                                    if !(0xdc00..=0xdfff).contains(&second) { return Err(self.error()); }
                                    0x10000 + ((first as u32 - 0xd800) << 10) + second as u32 - 0xdc00
                                } else { first as u32 };
                                char::from_u32(scalar).ok_or_else(|| self.error())?
                            }
                            _ => return Err(self.error()),
                        });
                    }
                    0..=31 => return Err(self.error()),
                    _ => {
                        let ch = self.text[self.at..].chars().next().ok_or_else(|| self.error())?;
                        out.push(ch); self.at += ch.len_utf8();
                    }
                }
            }
        }
        fn number(&mut self) -> Result<Json, String> {
            let start = self.at;
            if self.byte() == Some(b'-') { self.at += 1; }
            match self.byte() {
                Some(b'0') => self.at += 1,
                Some(b'1'..=b'9') => while matches!(self.byte(), Some(b'0'..=b'9')) { self.at += 1; },
                _ => return Err(self.error()),
            }
            if self.byte() == Some(b'.') {
                self.at += 1;
                let began = self.at;
                while matches!(self.byte(), Some(b'0'..=b'9')) { self.at += 1; }
                if began == self.at { return Err(self.error()); }
            }
            if matches!(self.byte(), Some(b'e' | b'E')) {
                self.at += 1;
                if matches!(self.byte(), Some(b'+' | b'-')) { self.at += 1; }
                let began = self.at;
                while matches!(self.byte(), Some(b'0'..=b'9')) { self.at += 1; }
                if began == self.at { return Err(self.error()); }
            }
            Ok(Json::Number(self.text[start..self.at].into()))
        }
        fn value(&mut self, depth: usize) -> Result<Json, String> {
            if depth > 64 { return Err("JSON nesting exceeds 64".into()); }
            self.whitespace();
            match self.byte() {
                Some(b'"') => self.string().map(Json::String),
                Some(b'-' | b'0'..=b'9') => self.number(),
                Some(b'{') => {
                    self.at += 1; self.whitespace();
                    let mut values = BTreeMap::new();
                    if self.byte() == Some(b'}') { self.at += 1; return Ok(Json::Object(values)); }
                    loop {
                        self.whitespace(); let key = self.string()?;
                        self.whitespace(); self.expect(b':')?;
                        if values.insert(key, self.value(depth + 1)?).is_some() { return Err("duplicate JSON object key".into()); }
                        self.whitespace();
                        if self.byte() == Some(b'}') { self.at += 1; break; }
                        self.expect(b',')?;
                    }
                    Ok(Json::Object(values))
                }
                Some(b'[') => {
                    self.at += 1; self.whitespace();
                    let mut values = Vec::new();
                    if self.byte() == Some(b']') { self.at += 1; return Ok(Json::Array(values)); }
                    loop {
                        values.push(self.value(depth + 1)?); self.whitespace();
                        if self.byte() == Some(b']') { self.at += 1; break; }
                        self.expect(b',')?;
                    }
                    Ok(Json::Array(values))
                }
                Some(_) => {
                    for (literal, value) in [("null", Json::Null), ("true", Json::Bool(true)), ("false", Json::Bool(false))] {
                        if self.text[self.at..].starts_with(literal) { self.at += literal.len(); return Ok(value); }
                    }
                    Err(self.error())
                }
                None => Err(self.error()),
            }
        }
    }
    let mut parser = Parser { text, at: 0 };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.at != text.len() { return Err(parser.error()); }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_escapes_numbers_and_roundtrip() {
        let value = parse(r#"{"路径":["\ud83d\ude80",null,true,-1.25e+2,9007199254740993,"\u0000\n\"\\"]}"#).unwrap();
        assert_eq!(parse(&value.encode()).unwrap(), value);
        assert!(value.encode().contains("🚀"));
        assert!(value.encode().contains("9007199254740993"));
    }
    #[test]
    fn malformed_and_ambiguous_json_is_rejected() {
        for text in [r#"{"x":1,"x":2}"#, r#""\ud800""#, r#""\udc00""#, "[1,]", "{\"x\":}", "01", "+1", "1.", "1e", "NaN", "true false", "\"\n\""] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert!(parse(&format!("{}0{}", "[".repeat(66), "]".repeat(66))).is_err());
    }
    #[test]
    fn integer_fields_are_exact_without_floating_point_rounding() {
        for (text, expected) in [("1.000", Some(1)), ("1e3", Some(1000)), ("0e99999999999999999999", Some(0)),
            ("9007199254740993", Some(9007199254740993)), ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775808", None), ("1.00000000000000000001", None), ("1e-999", None),
            ("4294967295.0", Some(4294967295)), ("-0.0000000000000000000000000001", None)] {
            assert_eq!(parse(text).unwrap().integer(), expected, "{text}");
        }
    }
}
