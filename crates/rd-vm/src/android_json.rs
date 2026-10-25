//! `android.util.JsonReader`, `JsonWriter` and `JsonToken`.
//!
//! The reader drains its `java.io.Reader` on first use (the same finite
//! source model as the host character readers), then runs the AOSP/Gson
//! streaming scope machine over the resulting text. The writer keeps its
//! scope stack in heap fields and emits each completed operation to the
//! wrapped guest/host `java.io.Writer`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const JSON_READER: &str = "Landroid/util/JsonReader;";
const JSON_WRITER: &str = "Landroid/util/JsonWriter;";
const JSON_TOKEN: &str = "Landroid/util/JsonToken;";

const READER_TEXT: &str = "rd_json_text";
const READER_BUF: &str = "rd_json_buf";
const READER_POS: &str = "rd_json_pos";
const READER_STACK: &str = "rd_json_stack";
const READER_PATH_NAMES: &str = "rd_json_path_names";
const READER_PATH_INDICES: &str = "rd_json_path_indices";
const READER_PEEKED: &str = "rd_json_peeked";
const READER_PEEKED_STRING: &str = "rd_json_peeked_string";
const JSON_LENIENT: &str = "rd_json_lenient";

const WRITER_STACK: &str = "rd_json_writer_stack";
const WRITER_INDENT: &str = "rd_json_writer_indent";
const WRITER_DEFERRED_NAME: &str = "rd_json_writer_deferred_name";
const WRITER_HTML_SAFE: &str = "rd_json_writer_html_safe";
const WRITER_SERIALIZE_NULLS: &str = "rd_json_writer_serialize_nulls";

fn throw(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}

fn get(vm: &Engine, recv: ObjRef, field: &str) -> Value {
    vm.heap.get_field(recv, field).unwrap_or(Value::Null)
}

fn put(vm: &mut Engine, recv: ObjRef, field: &str, value: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(recv, field, value))
}

fn string_value(vm: &Engine, value: &Value) -> Option<String> {
    match value {
        Value::Obj(reference) => vm.heap.as_str(*reference).ok().map(str::to_string),
        _ => None,
    }
}

fn string_field(vm: &Engine, recv: ObjRef, field: &str) -> Option<String> {
    string_value(vm, &get(vm, recv, field))
}

fn set_string_field(
    vm: &mut Engine,
    recv: ObjRef,
    field: &str,
    value: Option<&str>,
) -> Result<(), VmExit> {
    match (get(vm, recv, field), value) {
        (Value::Null, None) => return Ok(()),
        (Value::Obj(existing), Some(text)) if vm.heap.as_str(existing) == Ok(text) => {
            return Ok(());
        }
        _ => {}
    }
    let value = match value {
        Some(text) => Value::Obj(crate::intrinsics::alloc_string(vm, text.to_string())?),
        None => Value::Null,
    };
    put(vm, recv, field, value)
}

fn int_field(vm: &Engine, recv: ObjRef, field: &str, default: i32) -> i32 {
    match get(vm, recv, field) {
        Value::Int(value) => value,
        Value::Long(value) => value as i32,
        _ => default,
    }
}

fn long_field(vm: &Engine, recv: ObjRef, field: &str, default: i64) -> i64 {
    match get(vm, recv, field) {
        Value::Long(value) => value,
        Value::Int(value) => i64::from(value),
        _ => default,
    }
}

fn bool_field(vm: &Engine, recv: ObjRef, field: &str, default: bool) -> bool {
    match get(vm, recv, field) {
        Value::Int(value) => value != 0,
        _ => default,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    BeginArray,
    EndArray,
    BeginObject,
    EndObject,
    Name,
    String,
    Number,
    Boolean,
    Null,
    EndDocument,
}

const TOKENS: [Token; 10] = [
    Token::BeginArray,
    Token::EndArray,
    Token::BeginObject,
    Token::EndObject,
    Token::Name,
    Token::String,
    Token::Number,
    Token::Boolean,
    Token::Null,
    Token::EndDocument,
];

impl Token {
    fn name(self) -> &'static str {
        match self {
            Token::BeginArray => "BEGIN_ARRAY",
            Token::EndArray => "END_ARRAY",
            Token::BeginObject => "BEGIN_OBJECT",
            Token::EndObject => "END_OBJECT",
            Token::Name => "NAME",
            Token::String => "STRING",
            Token::Number => "NUMBER",
            Token::Boolean => "BOOLEAN",
            Token::Null => "NULL",
            Token::EndDocument => "END_DOCUMENT",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        TOKENS.into_iter().find(|token| token.name() == name)
    }

    fn ordinal(self) -> i32 {
        match self {
            Token::BeginArray => 0,
            Token::EndArray => 1,
            Token::BeginObject => 2,
            Token::EndObject => 3,
            Token::Name => 4,
            Token::String => 5,
            Token::Number => 6,
            Token::Boolean => 7,
            Token::Null => 8,
            Token::EndDocument => 9,
        }
    }

    fn from_ordinal(ordinal: i32) -> Option<Self> {
        usize::try_from(ordinal)
            .ok()
            .and_then(|index| TOKENS.get(index).copied())
    }
}

fn json_token_value(vm: &mut Engine, token: Token) -> Result<Value, VmExit> {
    let name = token.name();
    let key = (JSON_TOKEN.to_string(), name.to_string());
    if let Some(value @ Value::Obj(_)) = vm.statics.get(&key) {
        return Ok(value.clone());
    }
    let name_ref = crate::intrinsics::alloc_string(vm, name.to_string())?;
    let reference = vm
        .heap
        .alloc_instance(
            JSON_TOKEN.to_string(),
            vec![
                ("name".to_string(), Value::Obj(name_ref)),
                ("ordinal".to_string(), Value::Int(token.ordinal())),
            ],
        )
        .map_err(crate::err::oom_throwable)?;
    let value = Value::Obj(reference);
    vm.statics.insert(key, value.clone());
    Ok(value)
}

pub(crate) fn host_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if class != JSON_TOKEN {
        return Ok(None);
    }
    match Token::from_name(name) {
        Some(token) => Ok(Some(json_token_value(vm, token)?)),
        None => Ok(None),
    }
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != JSON_TOKEN {
        return Ok(None);
    }
    match name {
        "values" => {
            let mut values = Vec::with_capacity(TOKENS.len());
            for token in TOKENS {
                values.push(json_token_value(vm, token)?);
            }
            Ok(Some(crate::arrays::new_array(
                vm,
                "[Landroid/util/JsonToken;",
                ElemKind::Obj,
                values,
            )?))
        }
        "valueOf" => {
            let Some(wanted) = args.first().and_then(|value| string_value(vm, value)) else {
                return Err(throw("Ljava/lang/NullPointerException;", "Name is null"));
            };
            match Token::from_name(&wanted) {
                Some(token) => Ok(Some(json_token_value(vm, token)?)),
                None => Err(throw(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("No enum constant android.util.JsonToken.{wanted}"),
                )),
            }
        }
        _ => Ok(None),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    EmptyArray,
    NonemptyArray,
    EmptyObject,
    DanglingName,
    NonemptyObject,
    EmptyDocument,
    NonemptyDocument,
    Closed,
}

impl Scope {
    fn code(self) -> char {
        match self {
            Scope::EmptyArray => 'a',
            Scope::NonemptyArray => 'A',
            Scope::EmptyObject => 'o',
            Scope::DanglingName => 'n',
            Scope::NonemptyObject => 'O',
            Scope::EmptyDocument => 'd',
            Scope::NonemptyDocument => 'D',
            Scope::Closed => 'c',
        }
    }

    fn from_code(code: char) -> Option<Self> {
        Some(match code {
            'a' => Scope::EmptyArray,
            'A' => Scope::NonemptyArray,
            'o' => Scope::EmptyObject,
            'n' => Scope::DanglingName,
            'O' => Scope::NonemptyObject,
            'd' => Scope::EmptyDocument,
            'D' => Scope::NonemptyDocument,
            'c' => Scope::Closed,
            _ => return None,
        })
    }
}

fn encode_scopes(scopes: &[Scope]) -> String {
    scopes.iter().map(|scope| scope.code()).collect()
}

fn decode_scopes(encoded: Option<&str>) -> Vec<Scope> {
    let mut scopes: Vec<Scope> = encoded
        .unwrap_or("d")
        .chars()
        .filter_map(Scope::from_code)
        .collect();
    if scopes.is_empty() {
        scopes.push(Scope::EmptyDocument);
    }
    scopes
}

fn encode_path_names(names: &[Option<String>]) -> String {
    let mut encoded = String::new();
    for name in names {
        match name {
            Some(name) => {
                encoded.push_str(&name.chars().count().to_string());
                encoded.push(':');
                encoded.push_str(name);
            }
            None => encoded.push_str("-1:"),
        }
    }
    encoded
}

fn decode_path_names(encoded: Option<&str>, wanted: usize) -> Vec<Option<String>> {
    let mut names = Vec::new();
    let mut chars: Vec<char> = encoded.unwrap_or("").chars().collect();
    let mut pos = 0usize;
    while names.len() < wanted && pos < chars.len() {
        let colon = chars[pos..]
            .iter()
            .position(|ch| *ch == ':')
            .map(|offset| pos + offset);
        let Some(colon) = colon else {
            break;
        };
        let length_text: String = chars[pos..colon].iter().collect();
        let Ok(length) = length_text.parse::<i64>() else {
            break;
        };
        pos = colon + 1;
        if length < 0 {
            names.push(None);
        } else {
            let length = length as usize;
            let end = (pos + length).min(chars.len());
            names.push(Some(chars[pos..end].iter().collect()));
            pos = end;
        }
    }
    chars.clear();
    names.resize(wanted, None);
    names
}

#[derive(Clone, Debug)]
struct ReaderMeta {
    pos: usize,
    stack: Vec<Scope>,
    path_names: Vec<Option<String>>,
    path_indices: Vec<i32>,
    peeked: Option<Token>,
    peeked_string: Option<String>,
    lenient: bool,
}

impl ReaderMeta {
    fn new() -> Self {
        Self {
            pos: 0,
            stack: vec![Scope::EmptyDocument],
            path_names: vec![None],
            path_indices: vec![0],
            peeked: None,
            peeked_string: None,
            lenient: false,
        }
    }

    fn path(&self) -> String {
        let mut path = String::from("$");
        for (index, scope) in self.stack.iter().enumerate() {
            match scope {
                Scope::EmptyArray | Scope::NonemptyArray => {
                    let element = self.path_indices.get(index).copied().unwrap_or(0);
                    path.push('[');
                    path.push_str(&element.to_string());
                    path.push(']');
                }
                Scope::EmptyObject | Scope::DanglingName | Scope::NonemptyObject => {
                    if let Some(Some(name)) = self.path_names.get(index) {
                        path.push('.');
                        path.push_str(name);
                    }
                }
                Scope::EmptyDocument | Scope::NonemptyDocument | Scope::Closed => {}
            }
        }
        path
    }

    fn mark_closed(&mut self) {
        self.stack = vec![Scope::Closed];
        self.path_names = vec![None];
        self.path_indices = vec![0];
        self.peeked = None;
        self.peeked_string = None;
    }
}

#[derive(Clone, Debug)]
struct ReaderState<'a> {
    text: &'a str,
    pos: usize,
    stack: Vec<Scope>,
    path_names: Vec<Option<String>>,
    path_indices: Vec<i32>,
    peeked: Option<Token>,
    peeked_string: Option<String>,
    lenient: bool,
}

impl<'a> ReaderState<'a> {
    fn from_parts(text: &'a str, meta: ReaderMeta) -> Self {
        Self {
            text,
            pos: meta.pos,
            stack: meta.stack,
            path_names: meta.path_names,
            path_indices: meta.path_indices,
            peeked: meta.peeked,
            peeked_string: meta.peeked_string,
            lenient: meta.lenient,
        }
    }

    fn into_meta(self) -> ReaderMeta {
        ReaderMeta {
            pos: self.pos,
            stack: self.stack,
            path_names: self.path_names,
            path_indices: self.path_indices,
            peeked: self.peeked,
            peeked_string: self.peeked_string,
            lenient: self.lenient,
        }
    }

    fn location(&self) -> (usize, usize) {
        let mut line = 1usize;
        let mut column = 1usize;
        for ch in self.text[..self.pos.min(self.text.len())].chars() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    fn path(&self) -> String {
        let mut path = String::from("$");
        for (index, scope) in self.stack.iter().enumerate() {
            match scope {
                Scope::EmptyArray | Scope::NonemptyArray => {
                    let element = self.path_indices.get(index).copied().unwrap_or(0);
                    path.push('[');
                    path.push_str(&element.to_string());
                    path.push(']');
                }
                Scope::EmptyObject | Scope::DanglingName | Scope::NonemptyObject => {
                    if let Some(Some(name)) = self.path_names.get(index) {
                        path.push('.');
                        path.push_str(name);
                    }
                }
                Scope::EmptyDocument | Scope::NonemptyDocument | Scope::Closed => {}
            }
        }
        path
    }

    fn location_suffix(&self) -> String {
        let (line, column) = self.location();
        format!(" at line {line} column {column} path {}", self.path())
    }

    fn malformed(&self, message: &str) -> VmExit {
        throw(
            "Landroid/util/MalformedJsonException;",
            format!("{message}{}", self.location_suffix()),
        )
    }

    fn unexpected(&self, expected: Token, actual: Token) -> VmExit {
        throw(
            "Ljava/lang/IllegalStateException;",
            format!(
                "Expected {} but was {}{}",
                expected.name(),
                actual.name(),
                self.location_suffix()
            ),
        )
    }

    fn current(&self) -> Option<char> {
        self.text.get(self.pos..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.current()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn top(&self) -> Scope {
        self.stack.last().copied().unwrap_or(Scope::Closed)
    }

    fn set_top(&mut self, scope: Scope) {
        if let Some(top) = self.stack.last_mut() {
            *top = scope;
        }
    }

    fn push_scope(&mut self, scope: Scope) {
        self.stack.push(scope);
        self.path_names.push(None);
        self.path_indices.push(0);
    }

    fn skip_whitespace(&mut self) -> Result<Option<char>, VmExit> {
        loop {
            let Some(ch) = self.current() else {
                return Ok(None);
            };
            match ch {
                ' ' | '\t' | '\r' | '\n' => {
                    self.bump();
                }
                '/' if self.lenient => {
                    let rest = &self.text[self.pos..];
                    if rest.starts_with("//") {
                        self.pos += 2;
                        while let Some(next) = self.current() {
                            if next == '\n' || next == '\r' {
                                break;
                            }
                            self.bump();
                        }
                    } else if rest.starts_with("/*") {
                        let start = self.pos + 2;
                        let Some(end) = self.text[start..].find("*/") else {
                            return Err(self.malformed("Unterminated comment"));
                        };
                        self.pos = start + end + 2;
                    } else {
                        return Ok(Some(ch));
                    }
                }
                '#' if self.lenient => {
                    while let Some(next) = self.current() {
                        if next == '\n' || next == '\r' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => return Ok(Some(ch)),
            }
        }
    }

    fn peek(&mut self) -> Result<Token, VmExit> {
        if let Some(token) = self.peeked {
            return Ok(token);
        }
        let token = self.do_peek()?;
        self.peeked = Some(token);
        Ok(token)
    }

    fn do_peek(&mut self) -> Result<Token, VmExit> {
        let mut expected_name = false;
        match self.top() {
            Scope::Closed => {
                return Err(throw(
                    "Ljava/lang/IllegalStateException;",
                    "JsonReader is closed",
                ));
            }
            Scope::EmptyDocument => {
                if self.pos == 0 && self.current() == Some('\u{feff}') {
                    self.bump();
                }
                self.set_top(Scope::NonemptyDocument);
            }
            Scope::NonemptyDocument => {
                if self.skip_whitespace()?.is_none() {
                    return Ok(Token::EndDocument);
                }
                if !self.lenient {
                    return Err(self.malformed("Expected EOF"));
                }
            }
            Scope::EmptyArray => {
                self.set_top(Scope::NonemptyArray);
                if self.skip_whitespace()? == Some(']') {
                    self.bump();
                    return Ok(Token::EndArray);
                }
            }
            Scope::NonemptyArray => {
                match self.skip_whitespace()? {
                    Some(']') => {
                        self.bump();
                        return Ok(Token::EndArray);
                    }
                    Some(',') => {
                        self.bump();
                    }
                    Some(';') if self.lenient => {
                        self.bump();
                    }
                    None => return Err(self.malformed("End of input")),
                    _ => return Err(self.malformed("Expected ',' or ']'")),
                }
                if self.skip_whitespace()? == Some(']') {
                    if self.lenient {
                        self.bump();
                        return Ok(Token::EndArray);
                    }
                    return Err(self.malformed("Expected value"));
                }
            }
            Scope::EmptyObject => {
                self.set_top(Scope::DanglingName);
                if self.skip_whitespace()? == Some('}') {
                    self.bump();
                    return Ok(Token::EndObject);
                }
                expected_name = true;
            }
            Scope::NonemptyObject => {
                match self.skip_whitespace()? {
                    Some('}') => {
                        self.bump();
                        return Ok(Token::EndObject);
                    }
                    Some(',') => {
                        self.bump();
                    }
                    Some(';') if self.lenient => {
                        self.bump();
                    }
                    None => return Err(self.malformed("End of input")),
                    _ => return Err(self.malformed("Expected ',' or '}'")),
                }
                self.set_top(Scope::DanglingName);
                expected_name = true;
            }
            Scope::DanglingName => {
                self.set_top(Scope::NonemptyObject);
                match self.skip_whitespace()? {
                    Some(':') => {
                        self.bump();
                    }
                    Some('=') if self.lenient => {
                        self.bump();
                        if self.current() == Some('>') {
                            self.bump();
                        }
                    }
                    _ => return Err(self.malformed("Expected ':'")),
                }
            }
        }
        if expected_name {
            self.peek_name()
        } else {
            self.peek_value()
        }
    }

    fn peek_name(&mut self) -> Result<Token, VmExit> {
        match self.skip_whitespace()? {
            Some('"') => {
                self.peeked_string = Some(self.parse_string('"')?);
                Ok(Token::Name)
            }
            Some('\'') if self.lenient => {
                self.peeked_string = Some(self.parse_string('\'')?);
                Ok(Token::Name)
            }
            Some('}') => Err(self.malformed("Expected name")),
            Some(_) if self.lenient => {
                let literal = self.parse_unquoted_literal()?;
                if literal.is_empty() {
                    Err(self.malformed("Expected name"))
                } else {
                    self.peeked_string = Some(literal);
                    Ok(Token::Name)
                }
            }
            None => Err(self.malformed("End of input")),
            _ => Err(self.malformed("Expected name")),
        }
    }

    fn peek_value(&mut self) -> Result<Token, VmExit> {
        match self.skip_whitespace()? {
            Some('[') => {
                self.bump();
                Ok(Token::BeginArray)
            }
            Some('{') => {
                self.bump();
                Ok(Token::BeginObject)
            }
            Some('"') => {
                self.peeked_string = Some(self.parse_string('"')?);
                Ok(Token::String)
            }
            Some('\'') if self.lenient => {
                self.peeked_string = Some(self.parse_string('\'')?);
                Ok(Token::String)
            }
            Some(']') | Some('}') => Err(self.malformed("Expected value")),
            None => Err(self.malformed("End of input")),
            Some(_) => self.peek_literal(),
        }
    }

    fn parse_string(&mut self, quote: char) -> Result<String, VmExit> {
        self.bump(); // opening quote
        let mut units: Vec<u16> = Vec::new();
        loop {
            let Some(ch) = self.bump() else {
                return Err(self.malformed("Unterminated string"));
            };
            if ch == quote {
                return Ok(String::from_utf16_lossy(&units));
            }
            if ch == '\n' {
                return Err(self.malformed("Unterminated string"));
            }
            if ch == '\\' {
                let Some(escape) = self.bump() else {
                    return Err(self.malformed("Unterminated escape sequence"));
                };
                let escaped = match escape {
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let Some(hex) = self.bump() else {
                                return Err(self.malformed("Unterminated escape sequence"));
                            };
                            let Some(digit) = hex.to_digit(16) else {
                                return Err(self.malformed("Invalid escape sequence"));
                            };
                            code = (code << 4) + digit;
                        }
                        units.push(code as u16);
                        continue;
                    }
                    't' => '\t',
                    'b' => '\u{8}',
                    'n' => '\n',
                    'r' => '\r',
                    'f' => '\u{c}',
                    '\n' if self.lenient => continue,
                    '\'' if self.lenient || quote == '\'' => '\'',
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    _ => return Err(self.malformed("Invalid escape sequence")),
                };
                let mut scratch = [0u16; 2];
                units.extend_from_slice(escaped.encode_utf16(&mut scratch));
            } else if (ch as u32) < 0x20 && !self.lenient {
                return Err(self.malformed("Unescaped control character"));
            } else {
                let mut scratch = [0u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut scratch));
            }
        }
    }

    fn literal_delimiter(ch: char) -> bool {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ':' | ';' | '=' | '[' | ']' | '{' | '}' | '/' | '#'
            )
    }

    fn parse_unquoted_literal(&mut self) -> Result<String, VmExit> {
        let start = self.pos;
        while let Some(ch) = self.current() {
            if Self::literal_delimiter(ch) {
                break;
            }
            self.bump();
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn peek_literal(&mut self) -> Result<Token, VmExit> {
        let rest = &self.text[self.pos..];
        for (literal, token) in [
            ("true", Token::Boolean),
            ("false", Token::Boolean),
            ("null", Token::Null),
        ] {
            if let Some(stripped) = rest.strip_prefix(literal) {
                let next = stripped.chars().next();
                if next.is_none() || next.is_some_and(Self::literal_delimiter) {
                    self.pos += literal.len();
                    self.peeked_string = Some(literal.to_string());
                    return Ok(token);
                }
            }
        }
        let literal = self.parse_unquoted_literal()?;
        if literal.is_empty() {
            return Err(self.malformed("Expected value"));
        }
        if is_strict_number(&literal) || (self.lenient && parse_java_double(&literal).is_some()) {
            self.peeked_string = Some(literal);
            Ok(Token::Number)
        } else if self.lenient {
            self.peeked_string = Some(literal);
            Ok(Token::String)
        } else {
            Err(self.malformed("Expected value"))
        }
    }

    fn consume(&mut self, expected: Token) -> Result<Option<String>, VmExit> {
        let actual = self.peek()?;
        if actual != expected {
            return Err(self.unexpected(expected, actual));
        }
        Ok(self.take_peeked().1)
    }

    fn take_peeked(&mut self) -> (Token, Option<String>) {
        let token = self.peeked.take().unwrap_or(Token::EndDocument);
        let value = self.peeked_string.take();
        match token {
            Token::BeginArray => self.push_scope(Scope::EmptyArray),
            Token::BeginObject => self.push_scope(Scope::EmptyObject),
            Token::EndArray | Token::EndObject => {
                self.stack.pop();
                self.path_names.pop();
                self.path_indices.pop();
                if let Some(index) = self.path_indices.last_mut() {
                    *index = index.wrapping_add(1);
                }
            }
            Token::Name => {
                if let Some(slot) = self.path_names.last_mut() {
                    *slot = value.clone();
                }
            }
            Token::String | Token::Number | Token::Boolean | Token::Null => {
                if let Some(index) = self.path_indices.last_mut() {
                    *index = index.wrapping_add(1);
                }
            }
            Token::EndDocument => {}
        }
        (token, value)
    }

    fn next_string(&mut self) -> Result<String, VmExit> {
        let actual = self.peek()?;
        if actual != Token::String && actual != Token::Number {
            return Err(self.unexpected(Token::String, actual));
        }
        Ok(self.take_peeked().1.unwrap_or_default())
    }

    fn next_numeric_literal(&mut self) -> Result<String, VmExit> {
        let actual = self.peek()?;
        if actual != Token::String && actual != Token::Number {
            return Err(self.unexpected(Token::Number, actual));
        }
        Ok(self.take_peeked().1.unwrap_or_default())
    }

    fn next_double(&mut self) -> Result<f64, VmExit> {
        let literal = self.next_numeric_literal()?;
        let Some(value) = parse_java_double(&literal) else {
            return Err(throw(
                "Ljava/lang/NumberFormatException;",
                format!(
                    "Expected a double but was {literal}{}",
                    self.location_suffix()
                ),
            ));
        };
        if !self.lenient && (value.is_nan() || value.is_infinite()) {
            return Err(self.malformed(&format!("JSON forbids NaN and infinities: {value}")));
        }
        Ok(value)
    }

    fn next_long(&mut self) -> Result<i64, VmExit> {
        let literal = self.next_numeric_literal()?;
        parse_long(&literal).ok_or_else(|| {
            throw(
                "Ljava/lang/NumberFormatException;",
                format!(
                    "Expected a long but was {literal}{}",
                    self.location_suffix()
                ),
            )
        })
    }

    fn next_int(&mut self) -> Result<i32, VmExit> {
        let literal = self.next_numeric_literal()?;
        let value = parse_long(&literal).and_then(|value| i32::try_from(value).ok());
        value.ok_or_else(|| {
            throw(
                "Ljava/lang/NumberFormatException;",
                format!(
                    "Expected an int but was {literal}{}",
                    self.location_suffix()
                ),
            )
        })
    }

    fn skip_value(&mut self) -> Result<(), VmExit> {
        let mut depth = 0usize;
        loop {
            let token = self.peek()?;
            match token {
                Token::BeginArray | Token::BeginObject => {
                    self.take_peeked();
                    depth += 1;
                }
                Token::EndArray | Token::EndObject => {
                    if depth == 0 {
                        return Err(self.unexpected(Token::String, token));
                    }
                    self.take_peeked();
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Token::Name => {
                    self.take_peeked();
                }
                Token::String | Token::Number | Token::Boolean | Token::Null => {
                    self.take_peeked();
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Token::EndDocument => return Err(self.unexpected(Token::String, token)),
            }
        }
    }
}

fn is_strict_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index) {
        Some(b'0') => {
            index += 1;
        }
        Some(b'1'..=b'9') => {
            index += 1;
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

fn parse_java_double(value: &str) -> Option<f64> {
    match value {
        "NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return u64::from_str_radix(hex, 16)
                .ok()
                .map(|number| number as f64);
        }
    }
    if let Some(hex) = value
        .strip_prefix("-0x")
        .or_else(|| value.strip_prefix("-0X"))
    {
        if !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return u64::from_str_radix(hex, 16)
                .ok()
                .map(|number| -(number as f64));
        }
    }
    value.parse::<f64>().ok()
}

fn parse_long(value: &str) -> Option<i64> {
    if let Ok(parsed) = value.parse::<i64>() {
        return Some(parsed);
    }
    if !value.contains(['.', 'e', 'E']) {
        // Do not let a rounded double silently saturate an out-of-range
        // integer literal.
        return None;
    }
    let parsed = parse_java_double(value)?;
    if !parsed.is_finite() || parsed.fract() != 0.0 {
        return None;
    }
    if parsed < i64::MIN as f64 || parsed > i64::MAX as f64 {
        return None;
    }
    let result = parsed as i64;
    (result as f64 == parsed).then_some(result)
}

fn drain_reader_inner(vm: &mut Engine, reader: ObjRef, array: ObjRef) -> Result<String, VmExit> {
    const CHUNK_CHARS: usize = 4096;
    let mut text = String::new();
    let mut units: Vec<u16> = Vec::with_capacity(CHUNK_CHARS + 1);
    let mut pending_hi: Option<u16> = None;
    loop {
        let count = vm
            .invoke_instance(
                reader,
                "read",
                "([CII)I",
                &[
                    Value::Obj(array),
                    Value::Int(0),
                    Value::Int(CHUNK_CHARS as i32),
                ],
            )?
            .as_int()
            .unwrap_or(-1);
        if count == 0 {
            let ch = vm
                .invoke_instance(reader, "read", "()I", &[])?
                .as_int()
                .unwrap_or(-1);
            if ch < 0 {
                break;
            }
            units.clear();
            if let Some(hi) = pending_hi.take() {
                units.push(hi);
            }
            units.push(ch as u16);
        } else if count < 0 {
            break;
        } else {
            let elems = vm.heap.as_array_elems(array)?.0;
            let take = (count as usize).min(elems.len());
            units.clear();
            if let Some(hi) = pending_hi.take() {
                units.push(hi);
            }
            units.extend(
                elems[..take]
                    .iter()
                    .map(|value| value.as_int().unwrap_or(0) as u16),
            );
        }
        if let Some(&last) = units.last() {
            if (0xD800..=0xDBFF).contains(&last) {
                pending_hi = units.pop();
            }
        }
        let add_bytes: usize = char::decode_utf16(units.iter().copied())
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER).len_utf8())
            .sum();
        let projected = text
            .len()
            .saturating_add(add_bytes)
            .saturating_mul(2)
            .saturating_add(32);
        if vm.heap.used().saturating_add(projected) > vm.heap.budget() {
            return Err(crate::err::oom_throwable(crate::heap::OomError {
                requested: projected,
                budget: vm.heap.budget(),
            }));
        }
        text.reserve(add_bytes);
        for decoded in char::decode_utf16(units.iter().copied()) {
            text.push(decoded.unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }
    if pending_hi.is_some() {
        let add_bytes = char::REPLACEMENT_CHARACTER.len_utf8();
        let projected = text
            .len()
            .saturating_add(add_bytes)
            .saturating_mul(2)
            .saturating_add(32);
        if vm.heap.used().saturating_add(projected) > vm.heap.budget() {
            return Err(crate::err::oom_throwable(crate::heap::OomError {
                requested: projected,
                budget: vm.heap.budget(),
            }));
        }
        text.push(char::REPLACEMENT_CHARACTER);
    }
    Ok(text)
}

fn drain_reader(vm: &mut Engine, recv: ObjRef, reader: ObjRef) -> Result<String, VmExit> {
    let array = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, 4096, "[C")
        .map_err(crate::err::oom_throwable)?;
    put(vm, recv, READER_BUF, Value::Obj(array))?;
    let result = drain_reader_inner(vm, reader, array);
    let _ = put(vm, recv, READER_BUF, Value::Null);
    result
}

fn reader_load_meta(vm: &Engine, recv: ObjRef) -> ReaderMeta {
    let stack = decode_scopes(string_field(vm, recv, READER_STACK).as_deref());
    let mut path_names = decode_path_names(
        string_field(vm, recv, READER_PATH_NAMES).as_deref(),
        stack.len(),
    );
    let mut path_indices: Vec<i32> = string_field(vm, recv, READER_PATH_INDICES)
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(|value| value.parse().unwrap_or(0))
        .collect();
    path_names.resize(stack.len(), None);
    path_indices.resize(stack.len(), 0);
    ReaderMeta {
        pos: usize::try_from(long_field(vm, recv, READER_POS, 0)).unwrap_or(0),
        stack,
        path_names,
        path_indices,
        peeked: Token::from_ordinal(int_field(vm, recv, READER_PEEKED, -1)),
        peeked_string: string_field(vm, recv, READER_PEEKED_STRING),
        lenient: bool_field(vm, recv, JSON_LENIENT, false),
    }
}

fn reader_ensure_text(
    vm: &mut Engine,
    recv: ObjRef,
    meta: &ReaderMeta,
) -> Result<Option<ObjRef>, VmExit> {
    match get(vm, recv, READER_TEXT) {
        Value::Obj(existing) => Ok(Some(existing)),
        _ if meta.stack.last() != Some(&Scope::Closed) => {
            let Value::Obj(reader) = get(vm, recv, "in") else {
                return Err(throw("Ljava/lang/NullPointerException;", "in == null"));
            };
            let drained = drain_reader(vm, recv, reader)?;
            let text_ref = crate::intrinsics::alloc_string(vm, drained)?;
            put(vm, recv, READER_TEXT, Value::Obj(text_ref))?;
            Ok(Some(text_ref))
        }
        _ => Ok(None),
    }
}

fn reader_save_meta(vm: &mut Engine, recv: ObjRef, meta: &ReaderMeta) -> Result<(), VmExit> {
    put(
        vm,
        recv,
        READER_POS,
        Value::Long(i64::try_from(meta.pos).unwrap_or(i64::MAX)),
    )?;
    set_string_field(vm, recv, READER_STACK, Some(&encode_scopes(&meta.stack)))?;
    set_string_field(
        vm,
        recv,
        READER_PATH_NAMES,
        Some(&encode_path_names(&meta.path_names)),
    )?;
    let indices = meta
        .path_indices
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    set_string_field(vm, recv, READER_PATH_INDICES, Some(&indices))?;
    put(
        vm,
        recv,
        READER_PEEKED,
        Value::Int(meta.peeked.map_or(-1, Token::ordinal)),
    )?;
    set_string_field(
        vm,
        recv,
        READER_PEEKED_STRING,
        meta.peeked_string.as_deref(),
    )?;
    put(vm, recv, JSON_LENIENT, Value::Int(meta.lenient as i32))
}

enum ReaderOut {
    None,
    Bool(bool),
    Int(i32),
    Long(i64),
    Double(f64),
    Text(String),
    Token(Token),
}

fn json_reader(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(Ljava/io/Reader;)V") => {
            let Some(reader @ Value::Obj(_)) = args.first() else {
                return Err(throw("Ljava/lang/NullPointerException;", "in == null"));
            };
            put(vm, recv, "in", reader.clone())?;
            put(vm, recv, READER_TEXT, Value::Null)?;
            reader_save_meta(vm, recv, &ReaderMeta::new())?;
            return Ok(Some(Value::Null));
        }
        ("close", "()V") => {
            let mut meta = reader_load_meta(vm, recv);
            let reader = get(vm, recv, "in");
            meta.mark_closed();
            reader_save_meta(vm, recv, &meta)?;
            if let Value::Obj(reader) = reader {
                vm.invoke_instance(reader, "close", "()V", &[])?;
            }
            return Ok(Some(Value::Null));
        }
        ("setLenient", "(Z)V") => {
            let lenient = matches!(args.first(), Some(Value::Int(v)) if *v != 0);
            put(vm, recv, JSON_LENIENT, Value::Int(lenient as i32))?;
            return Ok(Some(Value::Null));
        }
        ("isLenient", "()Z") => {
            return Ok(Some(Value::Int(
                bool_field(vm, recv, JSON_LENIENT, false) as i32
            )));
        }
        _ => {}
    }

    if !matches!(
        (name, sig),
        ("beginArray", "()V")
            | ("endArray", "()V")
            | ("beginObject", "()V")
            | ("endObject", "()V")
            | ("hasNext", "()Z")
            | ("peek", "()Landroid/util/JsonToken;")
            | ("nextName", "()Ljava/lang/String;")
            | ("nextString", "()Ljava/lang/String;")
            | ("nextBoolean", "()Z")
            | ("nextNull", "()V")
            | ("nextDouble", "()D")
            | ("nextLong", "()J")
            | ("nextInt", "()I")
            | ("skipValue", "()V")
            | ("getPath", "()Ljava/lang/String;")
            | ("toString", "()Ljava/lang/String;")
    ) {
        return Ok(None);
    }

    if (name, sig) == ("getPath", "()Ljava/lang/String;") {
        let path = reader_load_meta(vm, recv).path();
        return Ok(Some(Value::Obj(crate::intrinsics::alloc_string(vm, path)?)));
    }
    if (name, sig) == ("toString", "()Ljava/lang/String;") {
        let meta = reader_load_meta(vm, recv);
        let text_out = {
            let text_slice = match get(vm, recv, READER_TEXT) {
                Value::Obj(r) => vm.heap.as_str(r).unwrap_or(""),
                _ => "",
            };
            let state = ReaderState::from_parts(text_slice, meta);
            format!("JsonReader{}", state.location_suffix())
        };
        return Ok(Some(Value::Obj(crate::intrinsics::alloc_string(
            vm, text_out,
        )?)));
    }

    let meta = reader_load_meta(vm, recv);
    let text_ref = reader_ensure_text(vm, recv, &meta)?;
    let (meta, result) = {
        let text_slice = text_ref.and_then(|r| vm.heap.as_str(r).ok()).unwrap_or("");
        let mut state = ReaderState::from_parts(text_slice, meta);
        let result = (|| -> Result<ReaderOut, VmExit> {
            Ok(match (name, sig) {
                ("beginArray", "()V") => {
                    state.consume(Token::BeginArray)?;
                    ReaderOut::None
                }
                ("endArray", "()V") => {
                    state.consume(Token::EndArray)?;
                    ReaderOut::None
                }
                ("beginObject", "()V") => {
                    state.consume(Token::BeginObject)?;
                    ReaderOut::None
                }
                ("endObject", "()V") => {
                    state.consume(Token::EndObject)?;
                    ReaderOut::None
                }
                ("hasNext", "()Z") => {
                    let token = state.peek()?;
                    ReaderOut::Bool(!matches!(
                        token,
                        Token::EndArray | Token::EndObject | Token::EndDocument
                    ))
                }
                ("peek", "()Landroid/util/JsonToken;") => ReaderOut::Token(state.peek()?),
                ("nextName", "()Ljava/lang/String;") => {
                    ReaderOut::Text(state.consume(Token::Name)?.unwrap_or_default())
                }
                ("nextString", "()Ljava/lang/String;") => ReaderOut::Text(state.next_string()?),
                ("nextBoolean", "()Z") => {
                    let value = state.consume(Token::Boolean)?.unwrap_or_default();
                    ReaderOut::Bool(value == "true")
                }
                ("nextNull", "()V") => {
                    state.consume(Token::Null)?;
                    ReaderOut::None
                }
                ("nextDouble", "()D") => ReaderOut::Double(state.next_double()?),
                ("nextLong", "()J") => ReaderOut::Long(state.next_long()?),
                ("nextInt", "()I") => ReaderOut::Int(state.next_int()?),
                ("skipValue", "()V") => {
                    state.skip_value()?;
                    ReaderOut::None
                }
                _ => ReaderOut::None,
            })
        })();
        (state.into_meta(), result)
    };
    match result {
        Ok(out) => {
            reader_save_meta(vm, recv, &meta)?;
            Ok(Some(match out {
                ReaderOut::None => Value::Null,
                ReaderOut::Bool(value) => Value::Int(value as i32),
                ReaderOut::Int(value) => Value::Int(value),
                ReaderOut::Long(value) => Value::Long(value),
                ReaderOut::Double(value) => Value::Double(value),
                ReaderOut::Text(text) => Value::Obj(crate::intrinsics::alloc_string(vm, text)?),
                ReaderOut::Token(token) => json_token_value(vm, token)?,
            }))
        }
        Err(error) => {
            let _ = reader_save_meta(vm, recv, &meta);
            Err(error)
        }
    }
}

#[derive(Clone, Debug)]
struct WriterState {
    stack: Vec<Scope>,
    indent: Option<String>,
    lenient: bool,
    html_safe: bool,
    serialize_nulls: bool,
    deferred_name: Option<String>,
    pending: String,
}

impl WriterState {
    fn new() -> Self {
        Self {
            stack: vec![Scope::EmptyDocument],
            indent: None,
            lenient: false,
            html_safe: false,
            serialize_nulls: true,
            deferred_name: None,
            pending: String::new(),
        }
    }

    fn top(&self) -> Scope {
        self.stack.last().copied().unwrap_or(Scope::Closed)
    }

    fn set_top(&mut self, scope: Scope) {
        if let Some(top) = self.stack.last_mut() {
            *top = scope;
        }
    }

    fn ensure_open(&self) -> Result<(), VmExit> {
        if self.top() == Scope::Closed {
            Err(throw(
                "Ljava/lang/IllegalStateException;",
                "JsonWriter is closed",
            ))
        } else {
            Ok(())
        }
    }

    fn newline(&mut self) {
        let Some(indent) = self.indent.clone() else {
            return;
        };
        self.pending.push('\n');
        for _ in 1..self.stack.len() {
            self.pending.push_str(&indent);
        }
    }

    fn before_name(&mut self) -> Result<(), VmExit> {
        match self.top() {
            Scope::EmptyObject => {
                self.set_top(Scope::DanglingName);
                self.newline();
                Ok(())
            }
            Scope::NonemptyObject => {
                self.pending.push(',');
                self.newline();
                self.set_top(Scope::DanglingName);
                Ok(())
            }
            _ => Err(throw(
                "Ljava/lang/IllegalStateException;",
                "Nesting problem",
            )),
        }
    }

    fn write_deferred_name(&mut self) -> Result<(), VmExit> {
        let Some(name) = self.deferred_name.take() else {
            return Ok(());
        };
        self.before_name()?;
        self.write_quoted(&name);
        Ok(())
    }

    fn before_value(&mut self) -> Result<(), VmExit> {
        self.ensure_open()?;
        match self.top() {
            Scope::EmptyArray => {
                self.set_top(Scope::NonemptyArray);
                self.newline();
            }
            Scope::NonemptyArray => {
                self.pending.push(',');
                self.newline();
            }
            Scope::DanglingName => {
                self.pending
                    .push_str(if self.indent.is_some() { ": " } else { ":" });
                self.set_top(Scope::NonemptyObject);
            }
            Scope::EmptyDocument => {
                self.set_top(Scope::NonemptyDocument);
            }
            Scope::NonemptyDocument => {
                if !self.lenient {
                    return Err(throw(
                        "Ljava/lang/IllegalStateException;",
                        "JSON must have only one top-level value.",
                    ));
                }
            }
            Scope::EmptyObject | Scope::NonemptyObject | Scope::Closed => {
                return Err(throw(
                    "Ljava/lang/IllegalStateException;",
                    "Nesting problem",
                ));
            }
        }
        Ok(())
    }

    fn open(&mut self, scope: Scope, bracket: char) -> Result<(), VmExit> {
        self.write_deferred_name()?;
        self.before_value()?;
        self.stack.push(scope);
        self.pending.push(bracket);
        Ok(())
    }

    fn close_scope(&mut self, empty: Scope, nonempty: Scope, bracket: char) -> Result<(), VmExit> {
        let scope = self.top();
        if scope != empty && scope != nonempty {
            return Err(throw(
                "Ljava/lang/IllegalStateException;",
                "Nesting problem",
            ));
        }
        if let Some(name) = &self.deferred_name {
            return Err(throw(
                "Ljava/lang/IllegalStateException;",
                format!("Dangling name: {name}"),
            ));
        }
        self.stack.pop();
        if scope == nonempty {
            self.newline();
        }
        self.pending.push(bracket);
        Ok(())
    }

    fn name(&mut self, name: String) -> Result<(), VmExit> {
        self.ensure_open()?;
        if self.deferred_name.is_some() {
            return Err(throw(
                "Ljava/lang/IllegalStateException;",
                "Nesting problem",
            ));
        }
        self.deferred_name = Some(name);
        Ok(())
    }

    fn null_value(&mut self) -> Result<(), VmExit> {
        self.ensure_open()?;
        if self.deferred_name.is_some() {
            if self.serialize_nulls {
                self.write_deferred_name()?;
            } else {
                self.deferred_name = None;
                return Ok(());
            }
        }
        self.before_value()?;
        self.pending.push_str("null");
        Ok(())
    }

    fn string_value(&mut self, value: &str) -> Result<(), VmExit> {
        self.write_deferred_name()?;
        self.before_value()?;
        self.write_quoted(value);
        Ok(())
    }

    fn raw_value(&mut self, value: &str) -> Result<(), VmExit> {
        self.write_deferred_name()?;
        self.before_value()?;
        self.pending.push_str(value);
        Ok(())
    }

    fn write_quoted(&mut self, value: &str) {
        self.pending.push('"');
        for ch in value.chars() {
            match ch {
                '"' => self.pending.push_str("\\\""),
                '\\' => self.pending.push_str("\\\\"),
                '\u{8}' => self.pending.push_str("\\b"),
                '\u{c}' => self.pending.push_str("\\f"),
                '\n' => self.pending.push_str("\\n"),
                '\r' => self.pending.push_str("\\r"),
                '\t' => self.pending.push_str("\\t"),
                '\u{2028}' => self.pending.push_str("\\u2028"),
                '\u{2029}' => self.pending.push_str("\\u2029"),
                '<' if self.html_safe => self.pending.push_str("\\u003c"),
                '>' if self.html_safe => self.pending.push_str("\\u003e"),
                '&' if self.html_safe => self.pending.push_str("\\u0026"),
                '=' if self.html_safe => self.pending.push_str("\\u003d"),
                '\'' if self.html_safe => self.pending.push_str("\\u0027"),
                ch if (ch as u32) < 0x20 => {
                    self.pending.push_str(&format!("\\u{:04x}", ch as u32));
                }
                ch => self.pending.push(ch),
            }
        }
        self.pending.push('"');
    }
}

fn writer_load(vm: &Engine, recv: ObjRef) -> WriterState {
    WriterState {
        stack: decode_scopes(string_field(vm, recv, WRITER_STACK).as_deref()),
        indent: string_field(vm, recv, WRITER_INDENT),
        lenient: bool_field(vm, recv, JSON_LENIENT, false),
        html_safe: bool_field(vm, recv, WRITER_HTML_SAFE, false),
        serialize_nulls: bool_field(vm, recv, WRITER_SERIALIZE_NULLS, true),
        deferred_name: string_field(vm, recv, WRITER_DEFERRED_NAME),
        pending: String::new(),
    }
}

fn writer_save(vm: &mut Engine, recv: ObjRef, state: &WriterState) -> Result<(), VmExit> {
    set_string_field(vm, recv, WRITER_STACK, Some(&encode_scopes(&state.stack)))?;
    set_string_field(vm, recv, WRITER_INDENT, state.indent.as_deref())?;
    set_string_field(
        vm,
        recv,
        WRITER_DEFERRED_NAME,
        state.deferred_name.as_deref(),
    )?;
    put(vm, recv, JSON_LENIENT, Value::Int(state.lenient as i32))?;
    put(
        vm,
        recv,
        WRITER_HTML_SAFE,
        Value::Int(state.html_safe as i32),
    )?;
    put(
        vm,
        recv,
        WRITER_SERIALIZE_NULLS,
        Value::Int(state.serialize_nulls as i32),
    )
}

fn writer_out_ref(vm: &Engine, recv: ObjRef) -> Result<ObjRef, VmExit> {
    match get(vm, recv, "out") {
        Value::Obj(out) => Ok(out),
        _ => Err(throw("Ljava/lang/NullPointerException;", "out == null")),
    }
}

fn writer_emit(vm: &mut Engine, recv: ObjRef, pending: &str) -> Result<(), VmExit> {
    if pending.is_empty() {
        return Ok(());
    }
    let out = writer_out_ref(vm, recv)?;
    let text = Value::Obj(crate::intrinsics::alloc_string(vm, pending.to_string())?);
    vm.invoke_instance(out, "write", "(Ljava/lang/String;)V", &[text])?;
    Ok(())
}

enum WriterOut {
    Null,
    This,
    Bool(bool),
}

fn json_writer(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(Ljava/io/Writer;)V") => {
            let Some(out @ Value::Obj(_)) = args.first() else {
                return Err(throw("Ljava/lang/NullPointerException;", "out == null"));
            };
            put(vm, recv, "out", out.clone())?;
            writer_save(vm, recv, &WriterState::new())?;
            return Ok(Some(Value::Null));
        }
        ("close", "()V") => {
            let mut state = writer_load(vm, recv);
            let complete = state.stack == [Scope::NonemptyDocument];
            let already_closed = state.top() == Scope::Closed;
            state.stack = vec![Scope::Closed];
            state.deferred_name = None;
            writer_save(vm, recv, &state)?;
            let out = writer_out_ref(vm, recv)?;
            vm.invoke_instance(out, "close", "()V", &[])?;
            if !already_closed && !complete {
                return Err(throw("Ljava/io/IOException;", "Incomplete document"));
            }
            return Ok(Some(Value::Null));
        }
        ("flush", "()V") => {
            let state = writer_load(vm, recv);
            state.ensure_open()?;
            let out = writer_out_ref(vm, recv)?;
            vm.invoke_instance(out, "flush", "()V", &[])?;
            return Ok(Some(Value::Null));
        }
        _ => {}
    }

    if !matches!(
        (name, sig),
        ("setIndent", "(Ljava/lang/String;)V")
            | ("setLenient", "(Z)V")
            | ("isLenient", "()Z")
            | ("setHtmlSafe", "(Z)V")
            | ("isHtmlSafe", "()Z")
            | ("setSerializeNulls", "(Z)V")
            | ("getSerializeNulls", "()Z")
            | ("beginArray", "()Landroid/util/JsonWriter;")
            | ("endArray", "()Landroid/util/JsonWriter;")
            | ("beginObject", "()Landroid/util/JsonWriter;")
            | ("endObject", "()Landroid/util/JsonWriter;")
            | ("name", "(Ljava/lang/String;)Landroid/util/JsonWriter;")
            | ("value", "(Ljava/lang/String;)Landroid/util/JsonWriter;")
            | ("value", "(Z)Landroid/util/JsonWriter;")
            | ("value", "(Ljava/lang/Boolean;)Landroid/util/JsonWriter;")
            | ("value", "(D)Landroid/util/JsonWriter;")
            | ("value", "(J)Landroid/util/JsonWriter;")
            | ("value", "(Ljava/lang/Number;)Landroid/util/JsonWriter;")
            | ("nullValue", "()Landroid/util/JsonWriter;")
    ) {
        return Ok(None);
    }

    // Resolve guest Number/Boolean arguments before applying writer state.
    let boolean_argument =
        if (name, sig) == ("value", "(Ljava/lang/Boolean;)Landroid/util/JsonWriter;") {
            match args.first() {
                Some(Value::Obj(reference)) => Some(
                    vm.invoke_instance(*reference, "booleanValue", "()Z", &[])?
                        .as_int()
                        .unwrap_or(0)
                        != 0,
                ),
                _ => None,
            }
        } else {
            None
        };
    let number_argument =
        if (name, sig) == ("value", "(Ljava/lang/Number;)Landroid/util/JsonWriter;") {
            match args.first() {
                Some(Value::Obj(reference)) => {
                    Some(crate::intrinsics::object_to_string(vm, *reference)?)
                }
                _ => None,
            }
        } else {
            None
        };

    let mut state = writer_load(vm, recv);
    let result = (|| -> Result<WriterOut, VmExit> {
        Ok(match (name, sig) {
            ("setIndent", "(Ljava/lang/String;)V") => {
                let Some(indent) = args.first().and_then(|value| string_value(vm, value)) else {
                    return Err(throw("Ljava/lang/NullPointerException;", "indent == null"));
                };
                state.indent = (!indent.is_empty()).then_some(indent);
                WriterOut::Null
            }
            ("setLenient", "(Z)V") => {
                state.lenient = matches!(args.first(), Some(Value::Int(1)));
                WriterOut::Null
            }
            ("isLenient", "()Z") => WriterOut::Bool(state.lenient),
            ("setHtmlSafe", "(Z)V") => {
                state.html_safe = matches!(args.first(), Some(Value::Int(1)));
                WriterOut::Null
            }
            ("isHtmlSafe", "()Z") => WriterOut::Bool(state.html_safe),
            ("setSerializeNulls", "(Z)V") => {
                state.serialize_nulls = matches!(args.first(), Some(Value::Int(1)));
                WriterOut::Null
            }
            ("getSerializeNulls", "()Z") => WriterOut::Bool(state.serialize_nulls),
            ("beginArray", "()Landroid/util/JsonWriter;") => {
                state.open(Scope::EmptyArray, '[')?;
                WriterOut::This
            }
            ("endArray", "()Landroid/util/JsonWriter;") => {
                state.close_scope(Scope::EmptyArray, Scope::NonemptyArray, ']')?;
                WriterOut::This
            }
            ("beginObject", "()Landroid/util/JsonWriter;") => {
                state.open(Scope::EmptyObject, '{')?;
                WriterOut::This
            }
            ("endObject", "()Landroid/util/JsonWriter;") => {
                state.close_scope(Scope::EmptyObject, Scope::NonemptyObject, '}')?;
                WriterOut::This
            }
            ("name", "(Ljava/lang/String;)Landroid/util/JsonWriter;") => {
                let Some(name) = args.first().and_then(|value| string_value(vm, value)) else {
                    return Err(throw("Ljava/lang/NullPointerException;", "name == null"));
                };
                state.name(name)?;
                WriterOut::This
            }
            ("value", "(Ljava/lang/String;)Landroid/util/JsonWriter;") => {
                match args.first().and_then(|value| string_value(vm, value)) {
                    Some(value) => state.string_value(&value)?,
                    None => state.null_value()?,
                }
                WriterOut::This
            }
            ("value", "(Z)Landroid/util/JsonWriter;") => {
                let value = matches!(args.first(), Some(Value::Int(1)));
                state.raw_value(if value { "true" } else { "false" })?;
                WriterOut::This
            }
            ("value", "(Ljava/lang/Boolean;)Landroid/util/JsonWriter;") => {
                match boolean_argument {
                    Some(value) => state.raw_value(if value { "true" } else { "false" })?,
                    None => state.null_value()?,
                }
                WriterOut::This
            }
            ("value", "(D)Landroid/util/JsonWriter;") => {
                let value = args
                    .first()
                    .and_then(|value| value.as_double().ok())
                    .unwrap_or(0.0);
                if !state.lenient && (value.is_nan() || value.is_infinite()) {
                    return Err(throw(
                        "Ljava/lang/IllegalArgumentException;",
                        format!("Numeric values must be finite, but was {value}"),
                    ));
                }
                state.raw_value(&crate::numerics::java_double_to_string(value, false))?;
                WriterOut::This
            }
            ("value", "(J)Landroid/util/JsonWriter;") => {
                let value = match args.first() {
                    Some(Value::Long(value)) => *value,
                    Some(value) => value.as_int().unwrap_or(0) as i64,
                    None => 0,
                };
                state.raw_value(&value.to_string())?;
                WriterOut::This
            }
            ("value", "(Ljava/lang/Number;)Landroid/util/JsonWriter;") => {
                match number_argument {
                    Some(value) => {
                        if !state.lenient
                            && matches!(value.as_str(), "NaN" | "Infinity" | "-Infinity")
                        {
                            return Err(throw(
                                "Ljava/lang/IllegalArgumentException;",
                                format!("Numeric values must be finite, but was {value}"),
                            ));
                        }
                        state.raw_value(&value)?;
                    }
                    None => state.null_value()?,
                }
                WriterOut::This
            }
            ("nullValue", "()Landroid/util/JsonWriter;") => {
                state.null_value()?;
                WriterOut::This
            }
            _ => WriterOut::Null,
        })
    })();
    match result {
        Ok(out) => {
            let pending = std::mem::take(&mut state.pending);
            writer_save(vm, recv, &state)?;
            writer_emit(vm, recv, &pending)?;
            Ok(Some(match out {
                WriterOut::Null => Value::Null,
                WriterOut::This => Value::Obj(recv),
                WriterOut::Bool(value) => Value::Int(value as i32),
            }))
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == JSON_READER || recv_class == JSON_READER {
        json_reader(vm, name, sig, recv, args)
    } else if mref_class == JSON_WRITER || recv_class == JSON_WRITER {
        json_writer(vm, name, sig, recv, args)
    } else {
        Ok(None)
    }
}
