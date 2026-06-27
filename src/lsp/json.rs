//! A minimal, total, `std`-only JSON parser for inbound LSP request bodies (Item D). Phorge has no
//! internal JSON parser — `Core.Json` is the *language's* parser, not callable here — and the
//! dependency policy forbids `serde` (an LSP server is not a security-critical primitive). This
//! parser handles exactly what LSP message bodies need: objects, arrays, strings (with escapes),
//! numbers, booleans, and null. It is internal tooling, off the byte-identity spine.

/// A parsed JSON value. Objects preserve key order (a `Vec` of pairs) — irrelevant for lookups but
/// avoids pulling in a map and keeps the type `Clone`/`Debug` trivially.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Parse a complete JSON document, or `None` on any malformed input (total — never panics).
    pub fn parse(input: &str) -> Option<Json> {
        let bytes = input.as_bytes();
        let mut p = ParseState { bytes, pos: 0 };
        p.skip_ws();
        let v = p.value()?;
        p.skip_ws();
        // Trailing junk after the top-level value is rejected.
        if p.pos == bytes.len() {
            Some(v)
        } else {
            None
        }
    }

    /// The value at object key `name`, or `None` if not an object / key absent.
    pub fn get(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, if this is a `Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The elements, if this is an `Arr`.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(xs) => Some(xs),
            _ => None,
        }
    }
}

struct ParseState<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl ParseState<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Option<Json> {
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' | b'f' => self.boolean(),
            b'n' => self.null(),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.bump(); // '{'
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Some(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bump() != Some(b':') {
                return None;
            }
            self.skip_ws();
            let val = self.value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Some(Json::Obj(pairs)),
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.bump(); // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Some(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Some(Json::Arr(items)),
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.bump(); // opening '"'
        let mut s = String::new();
        loop {
            match self.bump()? {
                b'"' => return Some(s),
                b'\\' => match self.bump()? {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b't' => s.push('\t'),
                    b'r' => s.push('\r'),
                    b'b' => s.push('\u{0008}'),
                    b'f' => s.push('\u{000C}'),
                    b'u' => {
                        let cp = self.hex4()?;
                        // Surrogate pair: a high surrogate must be followed by `\uXXXX` low surrogate.
                        if (0xD800..=0xDBFF).contains(&cp) {
                            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                return None;
                            }
                            let lo = self.hex4()?;
                            let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            s.push(char::from_u32(c)?);
                        } else {
                            s.push(char::from_u32(cp)?);
                        }
                    }
                    _ => return None,
                },
                // A multi-byte UTF-8 lead byte: copy its continuation bytes verbatim.
                b if b < 0x80 => s.push(b as char),
                b => {
                    let extra = match b {
                        0xC0..=0xDF => 1,
                        0xE0..=0xEF => 2,
                        0xF0..=0xF7 => 3,
                        _ => return None,
                    };
                    let start = self.pos - 1;
                    for _ in 0..extra {
                        self.bump()?;
                    }
                    s.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).ok()?);
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = (self.bump()? as char).to_digit(16)?;
            v = v * 16 + d;
        }
        Some(v)
    }

    fn boolean(&mut self) -> Option<Json> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Some(Json::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Some(Json::Bool(false))
        } else {
            None
        }
    }

    fn null(&mut self) -> Option<Json> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Some(Json::Null)
        } else {
            None
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        ) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()?
            .parse::<f64>()
            .ok()
            .map(Json::Num)
    }
}
