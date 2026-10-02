#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Data,
    Extern,
    Main,

    Pure,
    Impure,
    Unsafe,

    Return,
    Exit,

    To,
    As,

    TypeS16,
    TypeS32,
    TypeS64,
    TypeU8,
    TypeU16,
    TypeU32,
    TypeU64,
    TypeBool,

    Identifier(String),
    Integer(i64),
    Boolean(bool),
    String(Vec<u8>),

    Plus,
    PlusPlus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Hash,

    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    Arrow,

    Colon,
    Comma,

    LParen,
    RParen,
    LBracket,
    RBracket,

    Newline,
    Indent,
    Dedent,

    Eof,
}

pub struct Lexer {
    chars: Vec<char>,
    position: usize,
    at_line_start: bool,
    indent_stack: Vec<usize>,
}

impl Lexer {
    pub fn new(source: String) -> Self {
        Self {
            chars: source.chars().collect(),
            position: 0,
            at_line_start: true,
            indent_stack: vec![0],
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        while self.position < self.chars.len() {
            if self.at_line_start {
                if self.handle_indentation(&mut tokens)? {
                    continue;
                }
            }

            if self.position >= self.chars.len() {
                break;
            }

            let character = self.chars[self.position];

            match character {
                ' ' | '\t' | '\r' => {
                    self.position += 1;
                }

                '\n' => {
                    self.position += 1;
                    self.at_line_start = true;
                    tokens.push(Token::Newline);
                }

                ';' => {
                    self.skip_comment(&mut tokens);
                }

                '0'..='9' => {
                    tokens.push(self.lex_integer()?);
                }

                'a'..='z' | 'A'..='Z' | '_' => {
                    tokens.push(self.lex_identifier());
                }

                '"' => {
                    tokens.push(self.lex_string()?);
                }

                '+' => {
                    if self.peek_char(1) == Some('+') {
                        self.position += 2;
                        tokens.push(Token::PlusPlus);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Plus);
                    }
                }

                '-' => {
                    if self.peek_char(1) == Some('>') {
                        self.position += 2;
                        tokens.push(Token::Arrow);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Minus);
                    }
                }

                '*' => {
                    self.position += 1;
                    tokens.push(Token::Star);
                }

                '/' => {
                    self.position += 1;
                    tokens.push(Token::Slash);
                }

                '%' => {
                    self.position += 1;
                    tokens.push(Token::Percent);
                }

                '&' => {
                    self.position += 1;
                    tokens.push(Token::Ampersand);
                }

                '#' => {
                    self.position += 1;
                    tokens.push(Token::Hash);
                }

                '=' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::EqualEqual);
                    } else {
                        return Err("unexpected '='; use '==' for comparison".to_string());
                    }
                }

                '!' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::NotEqual);
                    } else {
                        return Err("unexpected '!'".to_string());
                    }
                }

                '<' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::LessEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Less);
                    }
                }

                '>' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::GreaterEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Greater);
                    }
                }

                ':' => {
                    self.position += 1;
                    tokens.push(Token::Colon);
                }

                ',' => {
                    self.position += 1;
                    tokens.push(Token::Comma);
                }

                '(' => {
                    self.position += 1;
                    tokens.push(Token::LParen);
                }

                ')' => {
                    self.position += 1;
                    tokens.push(Token::RParen);
                }

                '[' => {
                    self.position += 1;
                    tokens.push(Token::LBracket);
                }

                ']' => {
                    self.position += 1;
                    tokens.push(Token::RBracket);
                }

                other => {
                    return Err(format!(
                        "unexpected character '{}' at position {}",
                        other, self.position
                    ));
                }
            }
        }

        if !self.at_line_start {
            tokens.push(Token::Newline);
        }

        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            tokens.push(Token::Dedent);
        }

        tokens.push(Token::Eof);

        Ok(tokens)
    }

    fn handle_indentation(&mut self, tokens: &mut Vec<Token>) -> Result<bool, String> {
        let start = self.position;
        let mut indentation = 0usize;

        while self.position < self.chars.len() {
            match self.chars[self.position] {
                ' ' => {
                    indentation += 1;
                    self.position += 1;
                }

                '\t' => {
                    indentation += 4;
                    self.position += 1;
                }

                _ => break,
            }
        }

        if self.position >= self.chars.len() {
            self.at_line_start = false;
            return Ok(false);
        }

        if self.chars[self.position] == '\n' {
            self.position += 1;
            self.at_line_start = true;
            tokens.push(Token::Newline);
            return Ok(true);
        }

        if self.chars[self.position] == ';' {
            self.skip_comment(tokens);
            return Ok(true);
        }

        let current = *self
            .indent_stack
            .last()
            .expect("indentation stack must never be empty");

        if indentation > current {
            self.indent_stack.push(indentation);
            tokens.push(Token::Indent);
        } else if indentation < current {
            while indentation
                < *self
                    .indent_stack
                    .last()
                    .expect("indentation stack must never be empty")
            {
                self.indent_stack.pop();
                tokens.push(Token::Dedent);

                if self.indent_stack.is_empty() {
                    return Err("invalid indentation".to_string());
                }
            }

            let final_level = *self
                .indent_stack
                .last()
                .expect("indentation stack must never be empty");

            if indentation != final_level {
                return Err(format!(
                    "indentation does not match an outer indentation level at position {}",
                    start
                ));
            }
        }

        self.at_line_start = false;

        Ok(false)
    }

    fn skip_comment(&mut self, tokens: &mut Vec<Token>) {
        debug_assert_eq!(self.chars[self.position], ';');

        self.position += 1;

        while self.position < self.chars.len() {
            match self.chars[self.position] {
                '\n' => {
                    self.position += 1;
                    self.at_line_start = true;
                    tokens.push(Token::Newline);
                    return;
                }

                ';' => {
                    self.position += 1;
                    return;
                }

                _ => {
                    self.position += 1;
                }
            }
        }
    }

    fn lex_integer(&mut self) -> Result<Token, String> {
        let start = self.position;

        while self.position < self.chars.len() {
            match self.chars[self.position] {
                '0'..='9' => self.position += 1,
                _ => break,
            }
        }

        let text: String = self.chars[start..self.position].iter().collect();

        let value = text
            .parse::<i64>()
            .map_err(|_| format!("invalid integer '{}'", text))?;

        Ok(Token::Integer(value))
    }

    fn lex_identifier(&mut self) -> Token {
        let start = self.position;

        while self.position < self.chars.len() {
            match self.chars[self.position] {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                    self.position += 1;
                }

                _ => break,
            }
        }

        let text: String = self.chars[start..self.position].iter().collect();

        match text.as_str() {
            "data" => Token::Data,
            "extern" => Token::Extern,
            "main" => Token::Main,

            "pure" => Token::Pure,
            "impure" => Token::Impure,
            "unsafe" => Token::Unsafe,

            "return" => Token::Return,
            "exit" => Token::Exit,

            "to" => Token::To,
            "as" => Token::As,

            "s16" => Token::TypeS16,
            "s32" => Token::TypeS32,
            "s64" => Token::TypeS64,
            "u8" => Token::TypeU8,
            "u16" => Token::TypeU16,
            "u32" => Token::TypeU32,
            "u64" => Token::TypeU64,
            "bool" => Token::TypeBool,

            "true" => Token::Boolean(true),
            "false" => Token::Boolean(false),

            _ => Token::Identifier(text),
        }
    }

    fn lex_string(&mut self) -> Result<Token, String> {
        self.position += 1;

        let mut bytes = Vec::new();

        while self.position < self.chars.len() {
            let character = self.chars[self.position];

            match character {
                '"' => {
                    self.position += 1;
                    return Ok(Token::String(bytes));
                }

                '\\' => {
                    self.position += 1;

                    if self.position >= self.chars.len() {
                        return Err("unterminated string escape".to_string());
                    }

                    let escaped = self.chars[self.position];

                    match escaped {
                        '0' => bytes.push(0),
                        'n' => bytes.push(b'\n'),
                        'r' => bytes.push(b'\r'),
                        't' => bytes.push(b'\t'),
                        '\\' => bytes.push(b'\\'),
                        '"' => bytes.push(b'"'),

                        other => {
                            return Err(format!("unknown string escape '\\{}'", other));
                        }
                    }

                    self.position += 1;
                }

                '\n' => {
                    return Err("unterminated string literal".to_string());
                }

                other => {
                    if other as u32 > 0x7f {
                        return Err(format!("non-ASCII character '{}' in string literal", other));
                    }

                    bytes.push(other as u8);
                    self.position += 1;
                }
            }
        }

        Err("unterminated string literal".to_string())
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }
}
