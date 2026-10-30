use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Number(i64),
    List(Vec<Value>),
    Scope(HashMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    At,
    Ident(String),
    String(String),
    Number(i64),
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    Eof,
}

pub struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(&ch) = self.chars.peek() {
            if ch.is_whitespace() || ch == ',' {
                self.chars.next();
            } else if ch == '#' || ch == ';' {
                while let Some(&c) = self.chars.peek() {
                    self.chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Token, String> {
        self.skip_whitespace_and_comments();

        let ch = match self.chars.next() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match ch {
            '@' => Ok(Token::At),
            '(' => Ok(Token::OpenParen),
            ')' => Ok(Token::CloseParen),
            '[' => Ok(Token::OpenBracket),
            ']' => Ok(Token::CloseBracket),
            '"' => {
                let mut string = String::new();
                while let Some(c) = self.chars.next() {
                    if c == '"' {
                        return Ok(Token::String(string));
                    }
                    if c == '\\' {
                        if let Some(escaped) = self.chars.next() {
                            string.push(match escaped {
                                'n' => '\n',
                                't' => '\t',
                                'r' => '\r',
                                '"' => '"',
                                '\\' => '\\',
                                other => other,
                            });
                            continue;
                        }
                    }
                    string.push(c);
                }
                Err("unterminated string literal in yawn file".to_string())
            }
            c if c.is_ascii_digit() || c == '-' => {
                let mut s = String::new();
                s.push(c);
                while let Some(&next) = self.chars.peek() {
                    if next.is_ascii_digit() {
                        s.push(self.chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let n = s
                    .parse::<i64>()
                    .map_err(|_| format!("invalid integer: {}", s))?;
                Ok(Token::Number(n))
            }
            c if c.is_alphanumeric() || c == '_' => {
                let mut s = String::new();
                s.push(c);
                while let Some(&next) = self.chars.peek() {
                    if next.is_alphanumeric() || next == '_' || next == '-' {
                        s.push(self.chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                Ok(Token::Ident(s))
            }
            other => Err(format!("unexpected character in yawn: '{}'", other)),
        }
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.cursor).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
        self.tokens.get(self.cursor - 1).unwrap_or(&Token::Eof)
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let current = self.peek().clone();
        if current == expected {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, found {:?}", expected, current))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            token => Err(format!("expected identifier after '@', found {:?}", token)),
        }
    }

    fn parse_entry(&mut self) -> Result<(String, Value), String> {
        self.expect(Token::At)?;
        let key = self.expect_ident()?;
        let value = self.parse_value()?;
        Ok((key, value))
    }

    pub fn parse_value(&mut self) -> Result<Value, String> {
        match self.peek().clone() {
            Token::String(s) => {
                self.advance();
                Ok(Value::String(s))
            }
            Token::Number(n) => {
                self.advance();
                Ok(Value::Number(n))
            }
            Token::OpenBracket => self.parse_list(),
            Token::OpenParen => self.parse_scope(),
            token => Err(format!("expected value, found {:?}", token)),
        }
    }

    fn parse_list(&mut self) -> Result<Value, String> {
        self.expect(Token::OpenBracket)?;
        let mut items = Vec::new();
        while *self.peek() != Token::CloseBracket && *self.peek() != Token::Eof {
            items.push(self.parse_value()?);
        }
        self.expect(Token::CloseBracket)?;
        Ok(Value::List(items))
    }

    fn parse_scope(&mut self) -> Result<Value, String> {
        self.expect(Token::OpenParen)?;
        let mut scope = HashMap::new();
        while *self.peek() != Token::CloseParen && *self.peek() != Token::Eof {
            let (k, v) = self.parse_entry()?;
            scope.insert(k, v);
        }
        self.expect(Token::CloseParen)?;
        Ok(Value::Scope(scope))
    }

    pub fn parse_root(&mut self) -> Result<HashMap<String, Value>, String> {
        let mut root = HashMap::new();
        while *self.peek() != Token::Eof {
            let (k, v) = self.parse_entry()?;
            root.insert(k, v);
        }
        Ok(root)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRule {
    pub symbols: Vec<String>,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModConfig {
    pub exports: Vec<ExportRule>,
    pub imports: Vec<Vec<String>>,
}

impl ModConfig {
    pub fn parse_and_validate(source: &str, file_path: &str) -> Result<Self, String> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let token = lexer
                .next_token()
                .map_err(|e| format!("in '{}': {}", file_path, e))?;
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let mut parser = Parser::new(tokens);
        let root = parser
            .parse_root()
            .map_err(|e| format!("in '{}': {}", file_path, e))?;

        let mut exports = Vec::new();
        if let Some(export_val) = root.get("export") {
            let list = match export_val {
                Value::List(l) => l,
                other => {
                    return Err(format!(
                        "in '{}': @export must be a list, found {:?}",
                        file_path, other
                    ));
                }
            };

            for item in list {
                let scope = match item {
                    Value::Scope(s) => s,
                    other => {
                        return Err(format!(
                            "in '{}': each item in @export must be ( ... ), found {:?}",
                            file_path, other
                        ));
                    }
                };

                let symbol_val = scope.get("symbol").ok_or_else(|| {
                    format!("in '{}': missing '@symbol' in export group", file_path)
                })?;
                let to_val = scope
                    .get("to")
                    .ok_or_else(|| format!("in '{}': missing '@to' in export group", file_path))?;

                let symbols = extract_string_list(symbol_val, file_path, "@symbol")?;
                let to = extract_string_list(to_val, file_path, "@to")?;

                exports.push(ExportRule { symbols, to });
            }
        }

        let mut imports = Vec::new();
        if let Some(import_val) = root.get("import") {
            match import_val {
                Value::List(items) => {
                    for item in items {
                        let segs = extract_string_list(item, file_path, "each @import item")?;
                        imports.push(segs);
                    }
                }
                other => {
                    return Err(format!(
                        "in '{}': @import must be a list of lists, found {:?}",
                        file_path, other
                    ));
                }
            }
        }

        Ok(ModConfig { exports, imports })
    }
}

fn extract_string_list(val: &Value, path: &str, field: &str) -> Result<Vec<String>, String> {
    match val {
        Value::List(items) => {
            let mut result = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => result.push(s.clone()),
                    other => {
                        return Err(format!(
                            "in '{}': expected string in {}, found {:?}",
                            path, field, other
                        ));
                    }
                }
            }
            Ok(result)
        }
        other => Err(format!(
            "in '{}': expected list in {}, found {:?}",
            path, field, other
        )),
    }
}
