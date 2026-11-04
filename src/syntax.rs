use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

pub fn format_error(span: &Span, message: &str) -> String {
    if message.starts_with(&format!("{}:", span.file)) {
        message.to_string()
    } else {
        let contextual_message = message
            .strip_prefix(&format!("in '{}': ", span.file))
            .unwrap_or(message);
        format!(
            "{}:{}:{}: error: {}",
            span.file, span.line, span.col, contextual_message
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamespacePath {
    pub segments: Vec<String>,
}

impl NamespacePath {
    pub fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    pub fn from_segments(segments: Vec<String>) -> Self {
        Self { segments }
    }

    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn last_segment(&self) -> Option<&str> {
        self.segments.last().map(|s| s.as_str())
    }

    pub fn join(&self, name: &str) -> Self {
        let mut segments = self.segments.clone();
        segments.push(name.to_string());
        Self { segments }
    }
}

impl fmt::Display for NamespacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, ".")
        } else {
            for segment in &self.segments {
                write!(f, ".{}", segment)?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldType {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldInit {
    pub name: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Opaque,
    MetaType,
    Never,
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    USize,
    SSize,
    Bool,
    Atom(String),
    Alias(String),
    Struct(Vec<StructFieldType>),
    Pointer {
        element: Box<Type>,
    },
    Array {
        element: Box<Type>,
        size: usize,
    },
    Slice {
        element: Box<Type>,
    },
    Function {
        parameters: Vec<Type>,
        return_type: Box<Type>,
        is_variadic: bool,
    },
    Union(Vec<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub parameters: Vec<Parameter>,
    pub return_type: Box<Type>,
    pub is_variadic: bool,
}

pub type FunctionSignature = Signature;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Type,
    pub body: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IfCondition {
    Expression(Expression),
    Else,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfArm {
    pub condition: IfCondition,
    pub body: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub variant_type: Option<Type>,
    pub body: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Spanned {
        expression: Box<Expression>,
        span: Span,
    },

    // Primary Literals & Access
    Integer(i64),
    String(Vec<u8>),
    Boolean(bool),
    Atom(String),
    Identifier(String),
    Array(Vec<Expression>),
    Struct(Vec<StructFieldInit>),
    Record(Vec<StructFieldInit>),
    Member {
        expression: Box<Expression>,
        field: String,
    },
    Index {
        expression: Box<Expression>,
        index: Box<Expression>,
    },
    AddressOf(Box<Expression>),
    Dereference(Box<Expression>),
    Cast {
        expression: Box<Expression>,
        ty: Type,
    },
    Call {
        target: Box<Expression>,
        arguments: Vec<Expression>,
    },

    // Operators
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
    Comparison {
        left: Box<Expression>,
        operator: ComparisonOperator,
        right: Box<Expression>,
    },

    // Block & Value Control Flow
    Block(Vec<Expression>),
    Discard(Box<Expression>),
    Return(Box<Expression>),
    If {
        arms: Vec<IfArm>,
    },
    Match {
        target: Box<Expression>,
        binding_name: String,
        arms: Vec<MatchArm>,
    },

    // Binding & Mutation Expressions (evaluate to Type::none())
    Let {
        name: String,
        value: Box<Expression>,
    },
    Var {
        name: String,
        ty: Option<Type>,
        value: Option<Box<Expression>>,
    },
    Assign {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    Repeat {
        count: Box<Expression>,
        binder: Option<String>,
        body: Box<Expression>,
    },
    SizeOf(Type),
    Type(Type),
}

pub fn unspanned_expression(mut expression: &Expression) -> &Expression {
    while let Expression::Spanned {
        expression: inner, ..
    } = expression
    {
        expression = inner;
    }
    expression
}

pub fn unspanned_expression_mut(mut expression: &mut Expression) -> &mut Expression {
    while let Expression::Spanned {
        expression: inner, ..
    } = expression
    {
        expression = inner;
    }
    expression
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub signature: Option<Signature>,
    pub value: Expression,
    pub is_meta: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalFunction {
    pub name: String,
    pub signature: Signature,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    pub name: String,
    pub target: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub ty: Type,
    pub initializer: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub file_path: String,
    pub extern_functions: Vec<ExternalFunction>,
    pub definitions: Vec<Definition>,
    pub type_aliases: Vec<TypeAlias>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalModule {
    pub dir_path: String,
    pub source_files: Vec<SourceFile>,
    pub exports: Vec<crate::config::ExportRule>,
    pub imports: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub variables: Vec<Variable>,
    pub extern_functions: Vec<ExternalFunction>,
    pub definitions: Vec<Definition>,
    pub type_aliases: Vec<TypeAlias>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(Vec<u8>),
    Boolean(bool),
    Atom(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    ColonColon,
    ColonEqual,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Caret,
    Extern,
    Pipe,
    At,
    DotDot,
    DotDotDot,
    FatArrow,
    Arrow,
    Underscore,
    FnKeyword,
    TypeKeyword,
    ValKeyword,
    Var,
    Let,
    If,
    Else,
    Match,
    With,
    Using,
    Over,
    Return,
    As,
    Not,
    Repeat,
    Meta,
    TypeOpaque,
    Val,
    Never,
    TypeS16,
    TypeS32,
    TypeS64,
    TypeU8,
    TypeU16,
    TypeU32,
    TypeU64,
    TypeUSize,
    TypeSSize,
    TypeBool,
    Newline,
    Eof,
}

pub struct Lexer {
    chars: Vec<char>,
    locations: Vec<(usize, usize)>,
    position: usize,
    file_path: String,
}

impl Lexer {
    pub fn new(source: String) -> Self {
        Self::new_with_file(source, "<input>")
    }

    pub fn new_with_file(source: String, file_path: impl Into<String>) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let mut line = 1;
        let mut col = 1;
        let mut locations = Vec::with_capacity(chars.len() + 1);
        for character in &chars {
            locations.push((line, col));
            if *character == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        locations.push((line, col));
        Self {
            chars,
            locations,
            position: 0,
            file_path: file_path.into(),
        }
    }

    pub fn tokenize(self) -> Result<Vec<Token>, String> {
        self.tokenize_with_spans().map(|(tokens, _)| tokens)
    }

    pub fn tokenize_with_spans(mut self) -> Result<(Vec<Token>, Vec<Span>), String> {
        let file_path = self.file_path.clone();
        self.tokenize_inner()
            .map_err(|message| format_error(&self.span_at(self.position), &message))
            .map(|(tokens, spans)| {
                debug_assert!(spans.iter().all(|span| span.file == file_path));
                (tokens, spans)
            })
    }

    fn tokenize_inner(&mut self) -> Result<(Vec<Token>, Vec<Span>), String> {
        let mut tokens = Vec::new();
        let mut spans = Vec::new();

        while self.position < self.chars.len() {
            let token_start = self.position;
            let character = self.chars[self.position];

            match character {
                ' ' | '\t' | '\r' => {
                    self.position += 1;
                }
                '\n' => {
                    self.position += 1;
                    tokens.push(Token::Newline);
                }
                '@' => {
                    self.position += 1;
                    tokens.push(Token::At);
                }
                '{' => {
                    self.position += 1;
                    tokens.push(Token::LBrace);
                }
                '}' => {
                    self.position += 1;
                    tokens.push(Token::RBrace);
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
                    self.position += 1;
                    tokens.push(Token::Plus);
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
                '^' => {
                    self.position += 1;
                    tokens.push(Token::Caret);
                }
                '#' => {
                    self.skip_comment(&mut tokens);
                }
                '.' => {
                    if self.peek_char(1) == Some('.') && self.peek_char(2) == Some('.') {
                        self.position += 3;
                        tokens.push(Token::DotDotDot);
                    } else if self.peek_char(1) == Some('.') {
                        self.position += 2;
                        tokens.push(Token::DotDot);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Dot);
                    }
                }
                '|' => {
                    self.position += 1;
                    tokens.push(Token::Pipe);
                }
                '=' => {
                    if self.peek_char(1) == Some('>') {
                        self.position += 2;
                        tokens.push(Token::FatArrow);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Equal);
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
                    if self.peek_char(1) == Some(':') {
                        self.position += 2;
                        tokens.push(Token::ColonColon);
                    } else if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::ColonEqual);
                    } else if let Some(c) = self.peek_char(1) {
                        if c.is_alphabetic() || c == '_' {
                            tokens.push(self.lex_atom()?);
                        } else {
                            return Err(format!(
                                "expected atom identifier after ':', found '{}'",
                                c
                            ));
                        }
                    } else {
                        return Err("unexpected ':' at end of file".to_string());
                    }
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

            while spans.len() < tokens.len() {
                spans.push(self.span_at(token_start));
            }
        }

        spans.push(self.span_at(self.position));
        tokens.push(Token::Eof);
        Ok((tokens, spans))
    }

    fn span_at(&self, position: usize) -> Span {
        let (line, col) = self.locations[position.min(self.locations.len() - 1)];
        Span {
            file: self.file_path.clone(),
            line,
            col,
        }
    }

    fn skip_comment(&mut self, tokens: &mut Vec<Token>) {
        debug_assert_eq!(self.chars[self.position], '#');
        self.position += 1;

        while self.position < self.chars.len() {
            match self.chars[self.position] {
                '\n' => {
                    self.position += 1;
                    tokens.push(Token::Newline);
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

    fn lex_atom(&mut self) -> Result<Token, String> {
        debug_assert_eq!(self.chars[self.position], ':');
        self.position += 1;

        let start = self.position;
        while self.position < self.chars.len() {
            match self.chars[self.position] {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                    self.position += 1;
                }
                _ => break,
            }
        }

        let name: String = self.chars[start..self.position].iter().collect();
        if name.is_empty() {
            return Err("atom identifier cannot be empty".to_string());
        }

        Ok(Token::Atom(name))
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

        if text == "_" {
            return Token::Underscore;
        }

        match text.as_str() {
            "val" => Token::ValKeyword,
            "type" => Token::TypeKeyword,
            "extern" => Token::Extern,
            "fn" => Token::FnKeyword,
            "var" => Token::Var,
            "let" => Token::Let,
            "if" => Token::If,
            "else" => Token::Else,
            "match" => Token::Match,
            "with" => Token::With,
            "using" => Token::Using,
            "over" => Token::Over,
            "return" => Token::Return,
            "as" => Token::As,
            "s16" => Token::TypeS16,
            "s32" => Token::TypeS32,
            "s64" => Token::TypeS64,
            "u8" => Token::TypeU8,
            "u16" => Token::TypeU16,
            "u32" => Token::TypeU32,
            "u64" => Token::TypeU64,
            "usize" => Token::TypeUSize,
            "ssize" => Token::TypeSSize,
            "bool" => Token::TypeBool,
            "true" => Token::Boolean(true),
            "false" => Token::Boolean(false),
            "not" => Token::Not,
            "repeat" => Token::Repeat,
            "meta" => Token::Meta,
            "opaque" => Token::TypeOpaque,
            "never" => Token::Never,
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

pub struct Parser {
    tokens: Vec<Token>,
    spans: Vec<Span>,
    cursor: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let spans = vec![
            Span {
                file: "<input>".to_string(),
                line: 1,
                col: 1,
            };
            tokens.len()
        ];
        Self {
            tokens,
            spans,
            cursor: 0,
        }
    }

    pub fn new_with_spans(tokens: Vec<Token>, spans: Vec<Span>) -> Self {
        debug_assert_eq!(tokens.len(), spans.len());
        Self {
            tokens,
            spans,
            cursor: 0,
        }
    }

    pub fn current_span(&self) -> &Span {
        self.spans
            .get(self.cursor)
            .or_else(|| self.spans.last())
            .expect("parser requires an EOF token")
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.cursor).unwrap_or(&Token::Eof)
    }

    fn peek_non_newline(&self) -> &Token {
        let mut idx = self.cursor;
        while idx < self.tokens.len() {
            if self.tokens[idx] != Token::Newline {
                return &self.tokens[idx];
            }
            idx += 1;
        }
        &Token::Eof
    }

    fn advance(&mut self) -> &Token {
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
        self.tokens.get(self.cursor - 1).unwrap_or(&Token::Eof)
    }

    fn is(&self, token: Token) -> bool {
        self.peek() == &token
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        self.skip_newlines();
        let current = self.peek().clone();
        if current == expected {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, found {:?}", expected, current))
        }
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            Token::ValKeyword => {
                self.advance();
                Ok("val".to_string())
            }
            token => Err(format!("expected identifier, found {:?}", token)),
        }
    }

    fn expect_field_name(&mut self) -> Result<String, String> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            Token::ValKeyword => {
                self.advance();
                Ok("val".to_string())
            }
            Token::Val => {
                self.advance();
                Ok("val".to_string())
            }
            Token::Var => {
                self.advance();
                Ok("var".to_string())
            }
            Token::Let => {
                self.advance();
                Ok("let".to_string())
            }
            Token::If => {
                self.advance();
                Ok("if".to_string())
            }
            Token::Else => {
                self.advance();
                Ok("else".to_string())
            }
            Token::Match => {
                self.advance();
                Ok("match".to_string())
            }
            Token::With => {
                self.advance();
                Ok("with".to_string())
            }
            Token::Using => {
                self.advance();
                Ok("using".to_string())
            }
            Token::Over => {
                self.advance();
                Ok("over".to_string())
            }
            Token::Return => {
                self.advance();
                Ok("return".to_string())
            }
            Token::As => {
                self.advance();
                Ok("as".to_string())
            }
            Token::TypeS16 => {
                self.advance();
                Ok("s16".to_string())
            }
            Token::TypeS32 => {
                self.advance();
                Ok("s32".to_string())
            }
            Token::TypeS64 => {
                self.advance();
                Ok("s64".to_string())
            }
            Token::TypeU8 => {
                self.advance();
                Ok("u8".to_string())
            }
            Token::TypeU16 => {
                self.advance();
                Ok("u16".to_string())
            }
            Token::TypeU32 => {
                self.advance();
                Ok("u32".to_string())
            }
            Token::TypeU64 => {
                self.advance();
                Ok("u64".to_string())
            }
            Token::TypeUSize => {
                self.advance();
                Ok("usize".to_string())
            }
            Token::TypeSSize => {
                self.advance();
                Ok("ssize".to_string())
            }
            Token::Not => {
                self.advance();
                Ok("not".to_string())
            }
            Token::Repeat => {
                self.advance();
                Ok("repeat".to_string())
            }
            Token::Meta => {
                self.advance();
                Ok("meta".to_string())
            }
            Token::TypeOpaque => {
                self.advance();
                Ok("opaque".to_string())
            }
            Token::TypeBool => {
                self.advance();
                Ok("bool".to_string())
            }
            Token::TypeKeyword => {
                self.advance();
                Ok("type".to_string())
            }
            token => Err(format!("expected field name identifier, found {:?}", token)),
        }
    }

    fn skip_newlines(&mut self) {
        while self.is(Token::Newline) {
            self.advance();
        }
    }

    fn consume_optional_newline(&mut self) {
        if self.is(Token::Newline) {
            self.advance();
        }
    }

    pub fn parse_block(&mut self) -> Result<Expression, String> {
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut expressions = Vec::new();

        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
            let expr = self.parse_expression()?;

            if self.is(Token::RBrace) {
                // No trailing newline before '}' -> automatically return
                let ret_expr = match expr {
                    Expression::Return(_) => expr,
                    _ => Expression::Return(Box::new(expr)),
                };
                expressions.push(ret_expr);
                break;
            } else if self.is(Token::Newline) {
                // Trailing newline present -> evaluate, discard value, continue
                self.skip_newlines();
                expressions.push(Expression::Discard(Box::new(expr)));
            } else {
                return Err(format!(
                    "expected newline or '}}' after expression, found {:?}",
                    self.peek()
                ));
            }
        }

        self.expect(Token::RBrace)?;

        Ok(Expression::Block(expressions))
    }

    pub fn parse_file_declarations(
        &mut self,
        file_name: &str,
    ) -> Result<(Vec<ExternalFunction>, Vec<Definition>, Vec<TypeAlias>), String> {
        let mut extern_functions = Vec::new();
        let mut definitions = Vec::new();
        let mut type_aliases = Vec::new();

        self.skip_newlines();

        while !self.is(Token::Eof) {
            let declaration_span = self.current_span().clone();
            match self.peek() {
                Token::FnKeyword => {
                    let def = self.parse_fn_definition()?;
                    definitions.push(def);
                }
                Token::Meta => {
                    self.advance(); // consume 'meta'
                    self.skip_newlines();
                    if !self.is(Token::FnKeyword) {
                        return Err(format!("expected 'fn' after 'meta', found {:?}", self.peek()));
                    }
                    let mut def = self.parse_fn_definition()?;
                    def.is_meta = true;
                    definitions.push(def);
                }
                Token::TypeKeyword => {
                    self.advance(); // consume 'type'
                    self.skip_newlines();
                    let name = self.expect_identifier()?;
                    self.skip_newlines();
                    self.expect(Token::ColonEqual)?;
                    self.skip_newlines();
                    let target = self.parse_type()?;
                    self.consume_optional_newline();
                    type_aliases.push(TypeAlias {
                        name,
                        target,
                        span: declaration_span,
                    });
                }
                Token::ValKeyword => {
                    self.advance(); // consume 'val'
                    self.skip_newlines();
                    let name = self.expect_identifier()?;
                    self.skip_newlines();
                    self.expect(Token::ColonEqual)?;
                    self.skip_newlines();
                    let value = self.parse_expression()?;
                    self.consume_optional_newline();
                    definitions.push(Definition {
                        name,
                        signature: None,
                        value,
                        is_meta: false,
                    });
                }
                Token::Extern => {
                    self.advance(); // consume 'extern'
                    self.expect(Token::FnKeyword)?;
                    let name = self.expect_identifier()?;

                    self.expect(Token::LParen)?;
                    self.skip_newlines();
                    let mut parameters = Vec::new();
                    let mut is_variadic = false;

                    while !self.is(Token::RParen) && !self.is(Token::Eof) {
                        if self.is(Token::DotDotDot) {
                            self.advance();
                            is_variadic = true;
                            self.skip_newlines();
                            break;
                        }

                        let param_name = self.expect_field_name()?;
                        self.expect(Token::ColonColon)?;
                        self.skip_newlines();
                        let ty = self.parse_type()?;
                        parameters.push(Parameter {
                            name: param_name,
                            ty,
                        });
                        self.skip_newlines();

                        if self.is(Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(Token::RParen)?;

                    self.skip_newlines();
                    if !self.is(Token::Arrow) {
                        return Err("expected '->' followed by return type".to_string());
                    }
                    self.advance();
                    self.skip_newlines();
                    let return_type = self.parse_type()?;
                    self.consume_optional_newline();

                    let signature = Signature {
                        parameters,
                        return_type: Box::new(return_type),
                        is_variadic,
                    };

                    extern_functions.push(ExternalFunction {
                        name,
                        signature,
                        span: declaration_span,
                    });
                }
                Token::Newline => {
                    self.skip_newlines();
                }
                token => {
                    return Err(format!(
                        "in '{}': unexpected token at top level: {:?}. Declarations must start with 'fn', 'type', 'val', or 'extern'",
                        file_name, token
                    ));
                }
            }

            self.skip_newlines();
        }

        Ok((extern_functions, definitions, type_aliases))
    }

    fn parse_fn_definition(&mut self) -> Result<Definition, String> {
        self.advance(); // consume 'fn'
        let name = self.expect_identifier()?;

        self.expect(Token::LParen)?;
        self.skip_newlines();
        let mut parameters = Vec::new();
        while !self.is(Token::RParen) && !self.is(Token::Eof) {
            let param_name = self.expect_field_name()?;
            self.expect(Token::ColonColon)?;
            self.skip_newlines();
            let ty = self.parse_type()?;
            parameters.push(Parameter {
                name: param_name,
                ty,
            });
            self.skip_newlines();
            if self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(Token::RParen)?;

        self.skip_newlines();
        if !self.is(Token::Arrow) {
            return Err("expected '->' followed by return type".to_string());
        }
        self.advance();
        self.skip_newlines();
        let return_type = self.parse_type()?;

        self.skip_newlines();
        let body = self.parse_block()?;

        let signature = Signature {
            parameters,
            return_type: Box::new(return_type),
            is_variadic: false,
        };

        Ok(Definition {
            name,
            signature: Some(signature),
            value: body,
            is_meta: false,
        })
    }

    pub fn parse_expression(&mut self) -> Result<Expression, String> {
        let span = self.current_span().clone();
        let expression = self.parse_assignment()?;
        Ok(Expression::Spanned {
            expression: Box::new(expression),
            span,
        })
    }

    fn parse_assignment(&mut self) -> Result<Expression, String> {
        let expression = self.parse_comparison()?;

        if self.is(Token::ColonEqual) {
            self.advance();
            self.skip_newlines();
            let value = self.parse_assignment()?;
            return Ok(Expression::Assign {
                target: Box::new(expression),
                value: Box::new(value),
            });
        }

        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_additive()?;

        loop {
            if self.is(Token::Newline) {
                match self.peek_non_newline() {
                    Token::Equal
                    | Token::NotEqual
                    | Token::Less
                    | Token::LessEqual
                    | Token::Greater
                    | Token::GreaterEqual => {
                        self.skip_newlines();
                    }
                    _ => break,
                }
            }

            let operator = match self.peek() {
                Token::Equal => ComparisonOperator::Equal,
                Token::NotEqual => ComparisonOperator::NotEqual,
                Token::Less => ComparisonOperator::Less,
                Token::LessEqual => ComparisonOperator::LessEqual,
                Token::Greater => ComparisonOperator::Greater,
                Token::GreaterEqual => ComparisonOperator::GreaterEqual,
                _ => break,
            };

            self.advance();
            self.skip_newlines();
            let right = self.parse_additive()?;
            expression = Expression::Comparison {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_additive(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_multiplicative()?;

        loop {
            if self.is(Token::Newline) {
                match self.peek_non_newline() {
                    Token::Plus | Token::Minus => {
                        self.skip_newlines();
                    }
                    _ => break,
                }
            }

            let operator = match self.peek() {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Subtract,
                _ => break,
            };

            self.advance();
            self.skip_newlines();
            let right = self.parse_multiplicative()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_multiplicative(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_unary()?;

        loop {
            if self.is(Token::Newline) {
                match self.peek_non_newline() {
                    Token::Star | Token::Slash | Token::Percent => {
                        self.skip_newlines();
                    }
                    _ => break,
                }
            }

            let operator = match self.peek() {
                Token::Star => BinaryOperator::Multiply,
                Token::Slash => BinaryOperator::Divide,
                Token::Percent => BinaryOperator::Modulo,
                _ => break,
            };

            self.advance();
            self.skip_newlines();
            let right = self.parse_unary()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<Expression, String> {
        match self.peek() {
            Token::Plus => {
                self.advance();
                self.skip_newlines();
                Ok(Expression::Unary {
                    operator: UnaryOperator::Plus,
                    operand: Box::new(self.parse_unary()?),
                })
            }
            Token::Minus => {
                self.advance();
                self.skip_newlines();
                Ok(Expression::Unary {
                    operator: UnaryOperator::Minus,
                    operand: Box::new(self.parse_unary()?),
                })
            }
            Token::Not => {
                self.advance();
                self.skip_newlines();
                Ok(Expression::Unary {
                    operator: UnaryOperator::Not,
                    operand: Box::new(self.parse_unary()?),
                })
            }
            _ => {
                let expr = self.parse_primary()?;
                self.parse_postfix(expr)
            }
        }
    }

    fn parse_postfix(&mut self, mut expression: Expression) -> Result<Expression, String> {
        loop {
            if self.is(Token::Newline) {
                match self.peek_non_newline() {
                    Token::Dot | Token::DotDot | Token::As => {
                        self.skip_newlines();
                    }
                    _ => break,
                }
            }

            match self.peek().clone() {
                Token::LParen => {
                    self.advance();
                    let arguments = self.parse_argument_list(Token::RParen)?;
                    expression = Expression::Call {
                        target: Box::new(expression),
                        arguments,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    self.skip_newlines();
                    let index = self.parse_expression()?;
                    self.skip_newlines();
                    self.expect(Token::RBracket)?;
                    expression = Expression::Index {
                        expression: Box::new(expression),
                        index: Box::new(index),
                    };
                }
                Token::Dot => {
                    self.advance();
                    if self.is(Token::Star) {
                        self.advance();
                        expression = Expression::Dereference(Box::new(expression));
                    } else if self.is(Token::Ampersand) {
                        self.advance();
                        expression = Expression::AddressOf(Box::new(expression));
                    } else {
                        let field = self.expect_field_name()?;
                        expression = Expression::Member {
                            expression: Box::new(expression),
                            field,
                        };
                    }
                }
                Token::DotDot => {
                    self.advance();
                    self.skip_newlines();
                    let method_name = self.expect_identifier()?;
                    self.expect(Token::LParen)?;
                    let mut arguments = vec![expression];
                    let rest = self.parse_argument_list(Token::RParen)?;
                    arguments.extend(rest);
                    expression = Expression::Call {
                        target: Box::new(Expression::Identifier(method_name)),
                        arguments,
                    };
                }
                Token::As => {
                    self.advance();
                    self.skip_newlines();
                    let target_ty = self.parse_type()?;
                    expression = Expression::Cast {
                        expression: Box::new(expression),
                        ty: target_ty,
                    };
                }
                _ => break,
            }
        }

        Ok(expression)
    }
    fn parse_argument_list(&mut self, closing_token: Token) -> Result<Vec<Expression>, String> {
        let mut arguments = Vec::new();
        self.skip_newlines();

        while !self.is(closing_token.clone()) && !self.is(Token::Eof) {
            arguments.push(self.parse_expression()?);

            while self.is(Token::Comma) || self.is(Token::Newline) {
                self.advance();
            }
            self.skip_newlines();

            if self.is(closing_token.clone()) {
                break;
            }
        }

        self.expect(closing_token)?;
        Ok(arguments)
    }

    fn is_struct_type_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
            idx += 1;
        }
        if idx < self.tokens.len() {
            match &self.tokens[idx] {
                Token::Identifier(_)
                | Token::Val
                | Token::Var
                | Token::Let
                | Token::If
                | Token::Else
                | Token::Match
                | Token::With
                | Token::Using
                | Token::Over
                | Token::Return
                | Token::As
                | Token::Not
                | Token::Repeat
                | Token::Meta
                | Token::TypeOpaque
                | Token::TypeS16
                | Token::TypeS32
                | Token::TypeS64
                | Token::TypeU8
                | Token::TypeU16
                | Token::TypeU32
                | Token::TypeU64
                | Token::TypeUSize
                | Token::TypeSSize
                | Token::TypeBool => {
                    idx += 1;
                    while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
                        idx += 1;
                    }
                    return idx < self.tokens.len() && self.tokens[idx] == Token::ColonColon;
                }
                _ => {}
            }
        }
        false
    }

    fn is_struct_literal_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
            idx += 1;
        }
        if idx < self.tokens.len() {
            match &self.tokens[idx] {
                Token::Identifier(_)
                | Token::Val
                | Token::Var
                | Token::Let
                | Token::If
                | Token::Else
                | Token::Match
                | Token::With
                | Token::Using
                | Token::Over
                | Token::Return
                | Token::As
                | Token::Not
                | Token::Repeat
                | Token::Meta
                | Token::TypeOpaque
                | Token::TypeS16
                | Token::TypeS32
                | Token::TypeS64
                | Token::TypeU8
                | Token::TypeU16
                | Token::TypeU32
                | Token::TypeU64
                | Token::TypeUSize
                | Token::TypeSSize
                | Token::TypeBool => {
                    idx += 1;
                    while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
                        idx += 1;
                    }
                    return idx < self.tokens.len() && self.tokens[idx] == Token::ColonEqual;
                }
                _ => {}
            }
        }
        false
    }

    fn parse_primary(&mut self) -> Result<Expression, String> {
        match self.peek().clone() {
            Token::Integer(value) => {
                self.advance();
                Ok(Expression::Integer(value))
            }
            Token::Boolean(value) => {
                self.advance();
                Ok(Expression::Boolean(value))
            }
            Token::String(value) => {
                self.advance();
                Ok(Expression::String(value))
            }
            Token::Atom(name) => {
                self.advance();
                Ok(Expression::Atom(name))
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Expression::Identifier(name))
            }
            Token::Val | Token::ValKeyword => {
                self.advance();
                Ok(Expression::Identifier("val".to_string()))
            }
            Token::At => {
                self.advance();
                let tag = self.expect_identifier()?;
                self.skip_newlines();
                if tag == "sizeOf" {
                    let ty = self.parse_type()?;
                    return Ok(Expression::SizeOf(ty));
                }
                let is_record = tag == "record";
                if tag != "record" && tag != "struct" {
                    return Err(format!(
                        "unknown struct tag '@{}', expected '@record', '@struct', or '@sizeOf'",
                        tag
                    ));
                }
                self.expect(Token::LParen)?;
                let fields = self.parse_struct_fields(Token::RParen, is_record)?;
                if is_record {
                    Ok(Expression::Record(fields))
                } else {
                    Ok(Expression::Struct(fields))
                }
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();

                if self.is_struct_type_start() {
                    let mut fields = Vec::new();
                    while !self.is(Token::RParen) && !self.is(Token::Eof) {
                        let field_name = self.expect_field_name()?;
                        self.expect(Token::ColonColon)?;
                        self.skip_newlines();
                        let ty = self.parse_type()?;
                        fields.push(StructFieldType {
                            name: field_name,
                            ty,
                        });
                        self.skip_newlines();
                        if self.is(Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expression::Type(Type::Struct(fields)))
                } else if self.is_struct_literal_start() {
                    let fields = self.parse_struct_fields(Token::RParen, false)?;
                    Ok(Expression::Struct(fields))
                } else {
                    let expr = self.parse_expression()?;
                    self.expect(Token::RParen)?;
                    Ok(expr)
                }
            }
            Token::LBracket => {
                let is_slice_type = self.cursor + 1 < self.tokens.len()
                    && self.tokens[self.cursor + 1] == Token::RBracket
                    && self.cursor + 2 < self.tokens.len()
                    && matches!(
                        &self.tokens[self.cursor + 2],
                        Token::Identifier(_)
                            | Token::TypeS16
                            | Token::TypeS32
                            | Token::TypeS64
                            | Token::TypeU8
                            | Token::TypeU16
                            | Token::TypeU32
                            | Token::TypeU64
                            | Token::TypeUSize
                            | Token::TypeSSize
                            | Token::TypeBool
                            | Token::TypeOpaque
                            | Token::Star
                    );
                if is_slice_type {
                    let ty = self.parse_single_type()?;
                    Ok(Expression::Type(ty))
                } else {
                    self.advance();
                    let elements = self.parse_argument_list(Token::RBracket)?;
                    Ok(Expression::Array(elements))
                }
            }
            Token::TypeS16
            | Token::TypeS32
            | Token::TypeS64
            | Token::TypeU8
            | Token::TypeU16
            | Token::TypeU32
            | Token::TypeU64
            | Token::TypeUSize
            | Token::TypeSSize
            | Token::TypeBool
            | Token::TypeOpaque
            | Token::Star => {
                let ty = self.parse_single_type()?;
                Ok(Expression::Type(ty))
            }
            Token::LBrace => self.parse_block(),
            Token::If => self.parse_if_expression(),
            Token::Match => self.parse_match_expression(),
            Token::Let => {
                self.advance();
                let name = self.expect_identifier()?;
                if self.is(Token::ColonColon) {
                    return Err(format!(
                        "constants cannot be declared without an initialiser: 'let {} :: ...' is prohibited, use 'let {} := <value>'",
                        name, name
                    ));
                }
                self.expect(Token::ColonEqual)?;
                self.skip_newlines();
                let value = self.parse_expression()?;
                Ok(Expression::Let {
                    name,
                    value: Box::new(value),
                })
            }
            Token::Var => {
                self.advance();
                let name = self.expect_identifier()?;
                if self.is(Token::ColonColon) {
                    self.advance();
                    self.skip_newlines();
                    let ty = self.parse_type()?;
                    let value = if self.is(Token::ColonEqual) {
                        self.advance();
                        self.skip_newlines();
                        Some(Box::new(self.parse_expression()?))
                    } else {
                        None
                    };
                    Ok(Expression::Var {
                        name,
                        ty: Some(ty),
                        value,
                    })
                } else if self.is(Token::ColonEqual) {
                    self.advance();
                    self.skip_newlines();
                    let value = self.parse_expression()?;
                    Ok(Expression::Var {
                        name,
                        ty: None,
                        value: Some(Box::new(value)),
                    })
                } else {
                    Err(format!(
                        "expected '::' or ':=' after 'var {}', found {:?}",
                        name,
                        self.peek()
                    ))
                }
            }
            Token::Return => {
                self.advance();
                self.skip_newlines();
                let expr = self.parse_expression()?;
                Ok(Expression::Return(Box::new(expr)))
            }
            Token::Repeat => {
                self.advance();
                self.skip_newlines();
                let count = self.parse_expression()?;
                self.skip_newlines();
                let binder = if self.is(Token::Using) {
                    self.advance();
                    self.skip_newlines();
                    Some(self.expect_identifier()?)
                } else {
                    None
                };
                self.skip_newlines();
                self.expect(Token::FatArrow)?;
                self.skip_newlines();
                let body = self.parse_expression()?;
                Ok(Expression::Repeat {
                    count: Box::new(count),
                    binder,
                    body: Box::new(body),
                })
            }
            token => Err(format!("unexpected token in expression: {:?}", token)),
        }
    }

    pub fn parse_struct_fields(
        &mut self,
        closing_token: Token,
        is_record: bool,
    ) -> Result<Vec<StructFieldInit>, String> {
        let mut fields = Vec::new();
        self.skip_newlines();

        while !self.is(closing_token.clone()) && !self.is(Token::Eof) {
            self.skip_newlines();
            if self.is(closing_token.clone()) {
                break;
            }

            let name = self.expect_field_name()?;
            self.skip_newlines();
            self.expect(Token::ColonEqual)?;
            self.skip_newlines();

            let value = if self.is(Token::LParen) {
                self.advance();
                self.skip_newlines();
                let inner_fields = self.parse_struct_fields(Token::RParen, is_record)?;
                if is_record {
                    Expression::Record(inner_fields)
                } else {
                    Expression::Struct(inner_fields)
                }
            } else {
                self.parse_expression()?
            };

            fields.push(StructFieldInit { name, value });

            self.skip_newlines();
        }

        if closing_token != Token::Eof {
            self.expect(closing_token)?;
        }
        Ok(fields)
    }

    fn parse_match_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::Match)?;
        self.skip_newlines();

        let target = self.parse_expression()?;
        self.skip_newlines();

        if self.is(Token::Using) || self.is(Token::With) {
            self.advance();
        } else {
            return Err(format!("expected 'using', found {:?}", self.peek()));
        }
        self.skip_newlines();

        let binding_name = self.expect_identifier()?;
        self.skip_newlines();

        let has_brace = self.is(Token::LBrace);
        if has_brace {
            self.advance();
        }
        self.skip_newlines();

        let mut arms = Vec::new();

        let is_at_end = |p: &Parser| {
            if has_brace {
                p.is(Token::RBrace)
            } else {
                p.is(Token::Over)
            }
        };

        while !is_at_end(self) && !self.is(Token::Eof) {
            let variant_type = if self.is(Token::Else) {
                self.advance();
                None
            } else {
                Some(self.parse_type()?)
            };

            self.expect(Token::FatArrow)?;
            self.skip_newlines();

            let body = Box::new(self.parse_expression()?);
            arms.push(MatchArm { variant_type, body });
            self.skip_newlines();
            if self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }

        if has_brace {
            self.expect(Token::RBrace)?;
        } else {
            self.expect(Token::Over)?;
        }

        Ok(Expression::Match {
            target: Box::new(target),
            binding_name,
            arms,
        })
    }

    fn parse_if_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::If)?;
        self.skip_newlines();
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut arms = Vec::new();
        let mut has_else = false;

        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
            let condition = if self.is(Token::Else) {
                self.advance();
                has_else = true;
                IfCondition::Else
            } else {
                IfCondition::Expression(self.parse_expression()?)
            };

            self.expect(Token::FatArrow)?;
            self.skip_newlines();

            let body = Box::new(self.parse_expression()?);
            arms.push(IfArm { condition, body });
            self.skip_newlines();

            if self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }

            if has_else {
                break;
            }
        }

        self.expect(Token::RBrace)?;

        if arms.is_empty() {
            return Err("if block must contain at least one arm".to_string());
        }

        // Synthesize fallback: else => :none
        if !has_else {
            arms.push(IfArm {
                condition: IfCondition::Else,
                body: Box::new(Expression::Atom("none".to_string())),
            });
        }

        Ok(Expression::If { arms })
    }

    pub fn parse_type(&mut self) -> Result<Type, String> {
        let first = self.parse_single_type()?;
        self.skip_newlines();

        if self.is(Token::Pipe) {
            let mut variants = vec![first];
            while self.is(Token::Pipe) {
                self.advance();
                self.skip_newlines();
                variants.push(self.parse_single_type()?);
                self.skip_newlines();
            }
            Ok(Type::Union(variants))
        } else {
            Ok(first)
        }
    }

    fn parse_single_type(&mut self) -> Result<Type, String> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Never => {
                self.advance();
                Ok(Type::Never)
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Type::Alias(name))
            }
            Token::Val => {
                self.advance();
                Ok(Type::Alias("val".to_string()))
            }
            Token::Atom(name) => {
                self.advance();
                Ok(Type::Atom(name))
            }
            Token::TypeS16 => {
                self.advance();
                Ok(Type::S16)
            }
            Token::TypeS32 => {
                self.advance();
                Ok(Type::S32)
            }
            Token::TypeS64 => {
                self.advance();
                Ok(Type::S64)
            }
            Token::TypeU8 => {
                self.advance();
                Ok(Type::U8)
            }
            Token::TypeU16 => {
                self.advance();
                Ok(Type::U16)
            }
            Token::TypeU32 => {
                self.advance();
                Ok(Type::U32)
            }
            Token::TypeU64 => {
                self.advance();
                Ok(Type::U64)
            }
            Token::TypeUSize => {
                self.advance();
                Ok(Type::USize)
            }
            Token::TypeSSize => {
                self.advance();
                Ok(Type::SSize)
            }
            Token::TypeBool => {
                self.advance();
                Ok(Type::Bool)
            }
            Token::TypeOpaque => {
                self.advance();
                Ok(Type::Opaque)
            }
            Token::TypeKeyword => {
                self.advance();
                Ok(Type::MetaType)
            }
            Token::Star => {
                self.advance();
                self.skip_newlines();
                let inner = self.parse_single_type()?;
                Ok(Type::Pointer {
                    element: Box::new(inner),
                })
            }
            Token::LBracket => {
                self.advance();
                self.skip_newlines();
                if self.is(Token::RBracket) {
                    self.advance();
                    self.skip_newlines();
                    let inner = self.parse_single_type()?;
                    Ok(Type::Slice {
                        element: Box::new(inner),
                    })
                } else {
                    let size = match self.peek().clone() {
                        Token::Integer(value) if value >= 0 => {
                            self.advance();
                            value as usize
                        }
                        token => return Err(format!("expected array size, found {:?}", token)),
                    };
                    self.expect(Token::RBracket)?;
                    self.skip_newlines();
                    let inner = self.parse_single_type()?;
                    Ok(Type::Array {
                        element: Box::new(inner),
                        size,
                    })
                }
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();

                let mut fields = Vec::new();
                while !self.is(Token::RParen) && !self.is(Token::Eof) {
                    let field_name = self.expect_field_name()?;
                    self.expect(Token::ColonColon)?;
                    self.skip_newlines();
                    let ty = self.parse_type()?;
                    fields.push(StructFieldType {
                        name: field_name,
                        ty,
                    });
                    self.skip_newlines();
                    if self.is(Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(Token::RParen)?;
                Ok(Type::Struct(fields))
            }
            token => Err(format!("expected type, found {:?}", token)),
        }
    }
}
