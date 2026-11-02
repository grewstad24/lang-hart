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
    Void,
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
pub struct DoIfArm {
    pub condition: IfCondition,
    pub body: Box<Statement>,
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
    Integer(i64),
    String(Vec<u8>),
    Boolean(bool),
    Atom(String),
    Identifier(String),
    Array(Vec<Expression>),
    Struct(Vec<StructFieldInit>),
    Record(Vec<StructFieldInit>),
    Signature(Signature),
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
    Cast {
        expression: Box<Expression>,
        ty: Type,
    },
    Call {
        target: Box<Expression>,
        arguments: Vec<Expression>,
    },
    Index {
        expression: Box<Expression>,
        index: Box<Expression>,
    },
    Member {
        expression: Box<Expression>,
        field: String,
    },
    AddressOf(Box<Expression>),
    Dereference(Box<Expression>),
    Block {
        signature: Option<Signature>,
        body: Vec<Statement>,
    },
    If {
        arms: Vec<IfArm>,
    },
    Match {
        target: Box<Expression>,
        binding_name: String,
        arms: Vec<MatchArm>,
    },
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
pub enum Statement {
    Spanned {
        statement: Box<Statement>,
        span: Span,
    },
    Declare {
        name: String,
        ty: Type,
    },
    Assign {
        target: String,
        expression: Expression,
    },
    Discard(Expression),
    Return(Expression),
    Compound(Vec<Statement>),
    If {
        arms: Vec<DoIfArm>,
    },
    DoTimes {
        count: Expression,
        body: Box<Statement>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub value: Expression,
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
    At,
    Val,
    Var,
    Do,
    Done,
    If,
    DoIf,
    Else,
    End,
    Match,
    With,
    Over,
    Return,
    Not,
    DoTimes,
    To,
    As,
    Fn,
    Underscore,
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
    Atom(String),
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
    Caret,
    Hash,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Arrow,
    FatArrow,
    ColonColon,
    Comma,
    Dot,
    Pipe,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Newline,
    Eof,
    Backtick,
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
                ';' => {
                    self.skip_comment(&mut tokens);
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
                    tokens.push(self.lex_identifier(&tokens));
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
                '^' => {
                    self.position += 1;
                    tokens.push(Token::Caret);
                }
                '#' => {
                    self.position += 1;
                    tokens.push(Token::Hash);
                }
                '.' => {
                    self.position += 1;
                    tokens.push(Token::Dot);
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
                '`' => {
                    self.position += 1;
                    tokens.push(Token::Backtick);
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
        debug_assert_eq!(self.chars[self.position], ';');
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

    fn lex_identifier(&mut self, previous_tokens: &[Token]) -> Token {
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

        let is_after_at = previous_tokens
            .iter()
            .rev()
            .find(|t| !matches!(t, Token::Newline))
            == Some(&Token::At);

        match text.as_str() {
            "val" => {
                if is_after_at {
                    Token::Val
                } else {
                    Token::Identifier(text)
                }
            }
            "var" => Token::Var,
            "do" => Token::Do,
            "done" => Token::Done,
            "if" => Token::If,
            "doif" => Token::DoIf,
            "else" => Token::Else,
            "end" => Token::End,
            "match" => Token::Match,
            "with" => Token::With,
            "over" => Token::Over,
            "return" => Token::Return,
            "to" => Token::To,
            "as" => Token::As,
            "Fn" => Token::Fn,
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
            "_" => Token::Underscore,
            "dotimes" => Token::DoTimes,
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
            Token::Val => {
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
            Token::To => {
                self.advance();
                Ok("to".to_string())
            }
            Token::Val => {
                self.advance();
                Ok("val".to_string())
            }
            Token::Var => {
                self.advance();
                Ok("var".to_string())
            }
            Token::Do => {
                self.advance();
                Ok("do".to_string())
            }
            Token::Done => {
                self.advance();
                Ok("done".to_string())
            }
            Token::If => {
                self.advance();
                Ok("if".to_string())
            }
            Token::DoIf => {
                self.advance();
                Ok("doif".to_string())
            }
            Token::Else => {
                self.advance();
                Ok("else".to_string())
            }
            Token::End => {
                self.advance();
                Ok("end".to_string())
            }
            Token::Match => {
                self.advance();
                Ok("match".to_string())
            }
            Token::With => {
                self.advance();
                Ok("with".to_string())
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
            Token::Fn => {
                self.advance();
                Ok("Fn".to_string())
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
            Token::DoTimes => {
                self.advance();
                Ok("dotimes".to_string())
            }
            Token::TypeBool => {
                self.advance();
                Ok("bool".to_string())
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
                Token::At => {
                    self.advance();

                    let directive = match self.peek().clone() {
                        Token::Val => {
                            self.advance();
                            "val".to_string()
                        }
                        Token::Identifier(name) => {
                            self.advance();
                            name
                        }
                        token => {
                            return Err(format!(
                                "in '{}': expected directive after '@' (@val, @extern, @type, @main), found {:?}",
                                file_name, token
                            ));
                        }
                    };

                    match directive.as_str() {
                        "val" => {
                            self.skip_newlines();
                            let name = self.expect_identifier()?;
                            self.skip_newlines();
                            let value = self.parse_expression()?;
                            self.consume_optional_newline();
                            definitions.push(Definition { name, value });
                        }
                        "type" => {
                            self.skip_newlines();
                            let name = self.expect_identifier()?;
                            self.skip_newlines();
                            let target = self.parse_type()?;
                            self.consume_optional_newline();
                            type_aliases.push(TypeAlias {
                                name,
                                target,
                                span: declaration_span,
                            });
                        }
                        "extern" => {
                            self.skip_newlines();
                            let name = self.expect_identifier()?;
                            self.skip_newlines();
                            let expr = self.parse_expression()?;
                            self.consume_optional_newline();

                            if let Expression::Signature(signature) = unspanned_expression(&expr) {
                                extern_functions.push(ExternalFunction {
                                    name,
                                    signature: signature.clone(),
                                    span: declaration_span,
                                });
                            } else {
                                return Err(format!(
                                    "in '{}': expected signature for @extern '{}', found {:?}",
                                    file_name, name, expr
                                ));
                            }
                        }
                        "main" => {
                            self.skip_newlines();
                            let value = self.parse_expression()?;
                            self.consume_optional_newline();
                            definitions.push(Definition {
                                name: "main".to_string(),
                                value,
                            });
                        }
                        other => {
                            return Err(format!(
                                "in '{}': unknown top-level directive '@{}'",
                                file_name, other
                            ));
                        }
                    }
                }
                Token::Newline => {
                    self.skip_newlines();
                }
                token => {
                    return Err(format!(
                        "in '{}': unexpected token at top level: {:?}",
                        file_name, token
                    ));
                }
            }

            self.skip_newlines();
        }

        Ok((extern_functions, definitions, type_aliases))
    }

    pub fn parse_expression(&mut self) -> Result<Expression, String> {
        let span = self.current_span().clone();
        let expression = self.parse_comparison()?;
        Ok(Expression::Spanned {
            expression: Box::new(expression),
            span,
        })
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
            Token::Ampersand => {
                self.advance();
                self.skip_newlines();
                Ok(Expression::AddressOf(Box::new(self.parse_unary()?)))
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
                    Token::Dot | Token::Backtick | Token::As => {
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
                    let field = self.expect_field_name()?;
                    expression = Expression::Member {
                        expression: Box::new(expression),
                        field,
                    };
                }
                Token::Caret => {
                    self.advance();
                    expression = Expression::Dereference(Box::new(expression));
                }
                Token::Hash => {
                    if !matches!(expression, Expression::Struct(_)) {
                        return Err("'#record' can only follow a struct literal".to_string());
                    }
                    self.advance();
                    let name = self.expect_identifier()?;
                    if name != "record" {
                        return Err(format!("expected 'record' after '#', found '{}'", name));
                    }
                    let Expression::Struct(fields) = expression else {
                        unreachable!();
                    };
                    expression = Expression::Record(fields);
                }
                Token::Backtick => {
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

    pub fn parse_struct_fields(
        &mut self,
        closing_token: Token,
    ) -> Result<Vec<StructFieldInit>, String> {
        let mut fields = Vec::new();
        self.skip_newlines();

        while !self.is(closing_token.clone()) && !self.is(Token::Eof) {
            self.expect(Token::At)?;
            let name = self.expect_field_name()?;
            self.skip_newlines();
            let value = self.parse_expression()?;
            fields.push(StructFieldInit { name, value });

            while self.is(Token::Comma) || self.is(Token::Newline) {
                self.advance();
            }
            self.skip_newlines();

            if self.is(closing_token.clone()) {
                break;
            }
        }

        if closing_token != Token::Eof {
            self.expect(closing_token)?;
        }
        Ok(fields)
    }

    fn is_struct_literal_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() {
            if self.tokens[idx] != Token::Newline {
                return self.tokens[idx] == Token::At;
            }
            idx += 1;
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
            Token::Val => {
                self.advance();
                Ok(Expression::Identifier("val".to_string()))
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();

                if self.is_struct_literal_start() {
                    let fields = self.parse_struct_fields(Token::RParen)?;
                    return Ok(Expression::Struct(fields));
                }

                if self.is_signature_start() {
                    let sig = self.parse_signature_inner()?;
                    self.expect(Token::RParen)?;
                    self.skip_newlines();
                    let is_variadic = if self.is(Token::PlusPlus) {
                        self.advance();
                        self.skip_newlines();
                        true
                    } else {
                        false
                    };
                    let return_type = if self.is(Token::To) {
                        self.advance();
                        self.skip_newlines();
                        Box::new(self.parse_type()?)
                    } else {
                        Box::new(Type::Void)
                    };
                    Ok(Expression::Signature(Signature {
                        parameters: sig,
                        return_type,
                        is_variadic,
                    }))
                } else {
                    let expr = self.parse_expression()?;
                    self.expect(Token::RParen)?;
                    Ok(expr)
                }
            }
            Token::LBracket => {
                self.advance();
                let elements = self.parse_argument_list(Token::RBracket)?;
                Ok(Expression::Array(elements))
            }
            Token::Do => self.parse_block(),
            Token::If => self.parse_if_expression(),
            Token::Match => self.parse_match_expression(),
            token => Err(format!("unexpected token in expression: {:?}", token)),
        }
    }

    fn parse_match_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::Match)?;
        self.skip_newlines();

        let target = self.parse_expression()?;
        self.skip_newlines();

        self.expect(Token::With)?;
        self.skip_newlines();

        let binding_name = self.expect_identifier()?;
        self.skip_newlines();

        let mut arms = Vec::new();

        while !self.is(Token::Over) && !self.is(Token::Eof) {
            let variant_type = if self.is(Token::Underscore) {
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
        }

        self.expect(Token::Over)?;

        Ok(Expression::Match {
            target: Box::new(target),
            binding_name,
            arms,
        })
    }

    fn is_signature_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() {
            if self.tokens[idx] == Token::ColonColon {
                return true;
            }
            if self.tokens[idx] == Token::RParen || self.tokens[idx] == Token::Eof {
                break;
            }
            idx += 1;
        }
        false
    }

    fn parse_signature_inner(&mut self) -> Result<Vec<Parameter>, String> {
        let mut parameters = Vec::new();
        self.skip_newlines();

        while !self.is(Token::RParen) && !self.is(Token::Eof) {
            let name = self.expect_field_name()?;
            self.expect(Token::ColonColon)?;
            self.skip_newlines();
            let ty = self.parse_type()?;
            parameters.push(Parameter { name, ty });

            while self.is(Token::Comma) || self.is(Token::Newline) {
                self.advance();
            }
            self.skip_newlines();

            if self.is(Token::RParen) {
                break;
            }
        }

        Ok(parameters)
    }

    fn parse_block(&mut self) -> Result<Expression, String> {
        self.expect(Token::Do)?;
        self.skip_newlines();

        let signature = if self.is(Token::LParen) || self.is(Token::To) {
            let mut parameters = Vec::new();
            if self.is(Token::LParen) {
                self.advance();
                parameters = self.parse_signature_inner()?;
                self.expect(Token::RParen)?;
                self.skip_newlines();
            }

            let return_type = if self.is(Token::To) {
                self.advance();
                self.skip_newlines();
                Box::new(self.parse_type()?)
            } else {
                Box::new(Type::Void)
            };

            Some(Signature {
                parameters,
                return_type,
                is_variadic: false,
            })
        } else {
            None
        };

        self.skip_newlines();

        let mut body = Vec::new();
        while !self.is(Token::Done) && !self.is(Token::Eof) {
            body.push(self.parse_statement()?);
            self.skip_newlines();
        }

        self.expect(Token::Done)?;

        Ok(Expression::Block { signature, body })
    }

    fn parse_if_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::If)?;
        self.skip_newlines();

        let mut arms = Vec::new();
        let mut has_else = false;

        while !self.is(Token::End) && !self.is(Token::Eof) {
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

            if has_else {
                break;
            }
        }

        self.expect(Token::End)?;

        if !has_else {
            return Err("expression 'if' requires a fallback 'else' arm".to_string());
        }

        Ok(Expression::If { arms })
    }

    pub fn parse_statement(&mut self) -> Result<Statement, String> {
        let span = self.current_span().clone();
        let statement = self.parse_statement_inner()?;
        Ok(Statement::Spanned {
            statement: Box::new(statement),
            span,
        })
    }

    fn parse_statement_inner(&mut self) -> Result<Statement, String> {
        match self.peek() {
            Token::LBrace => {
                self.advance();
                self.skip_newlines();
                let mut body = Vec::new();
                while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                    body.push(self.parse_statement()?);
                    self.skip_newlines();
                }
                self.expect(Token::RBrace)?;
                self.consume_optional_newline();
                Ok(Statement::Compound(body))
            }
            Token::Var => {
                self.advance();
                let name = self.expect_identifier()?;
                self.expect(Token::ColonColon)?;
                self.skip_newlines();
                let ty = self.parse_type()?;
                self.consume_optional_newline();
                Ok(Statement::Declare { name, ty })
            }
            Token::DoIf => self.parse_doif_statement(),
            Token::DoTimes => {
                self.advance();
                self.skip_newlines();
                let count = self.parse_expression()?;
                self.expect(Token::FatArrow)?;
                self.skip_newlines();
                let body = self.parse_statement()?;
                self.consume_optional_newline();
                Ok(Statement::DoTimes {
                    count,
                    body: Box::new(body),
                })
            }
            Token::Return => {
                self.advance();
                self.skip_newlines();
                let expr = self.parse_expression()?;
                self.consume_optional_newline();
                Ok(Statement::Return(expr))
            }
            _ => {
                let expression = self.parse_expression()?;

                if self.is(Token::Newline) && self.peek_non_newline() == &Token::Arrow {
                    self.skip_newlines();
                }

                if self.is(Token::Arrow) {
                    self.advance();
                    self.skip_newlines();
                    let target = self.expect_identifier()?;
                    if self.is(Token::Arrow) {
                        return Err(
                            "multiple assignment via chained '->' is not supported".to_string()
                        );
                    }
                    self.consume_optional_newline();
                    Ok(Statement::Assign { expression, target })
                } else {
                    self.consume_optional_newline();
                    Ok(Statement::Discard(expression))
                }
            }
        }
    }

    fn parse_doif_statement(&mut self) -> Result<Statement, String> {
        self.expect(Token::DoIf)?;
        self.skip_newlines();

        let mut arms = Vec::new();
        let mut has_else = false;

        while !self.is(Token::End) && !self.is(Token::Eof) {
            let condition = if self.is(Token::Else) {
                self.advance();
                has_else = true;
                IfCondition::Else
            } else {
                IfCondition::Expression(self.parse_expression()?)
            };

            self.expect(Token::FatArrow)?;
            self.skip_newlines();

            let body = self.parse_statement()?;
            arms.push(DoIfArm {
                condition,
                body: Box::new(body),
            });
            self.skip_newlines();

            if has_else {
                break;
            }
        }

        self.expect(Token::End)?;
        self.consume_optional_newline();

        if !has_else {
            arms.push(DoIfArm {
                condition: IfCondition::Else,
                body: Box::new(Statement::Compound(Vec::new())),
            });
        }

        Ok(Statement::If { arms })
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
            Token::Fn => {
                self.advance();
                self.skip_newlines();
                self.expect(Token::LBrace)?;
                self.skip_newlines();

                let mut parameters = Vec::new();
                if self.is(Token::LParen) {
                    self.advance();
                    self.skip_newlines();
                    if !self.is(Token::RParen) {
                        parameters = self.parse_signature_inner()?;
                    }
                    self.expect(Token::RParen)?;
                }

                self.skip_newlines();

                let is_variadic = if self.is(Token::PlusPlus) {
                    self.advance();
                    self.skip_newlines();
                    true
                } else {
                    false
                };

                let return_type = if self.is(Token::To) {
                    self.advance();
                    self.skip_newlines();
                    Box::new(self.parse_type()?)
                } else {
                    Box::new(Type::Void)
                };

                self.skip_newlines();
                self.expect(Token::RBrace)?;

                let param_types = parameters.into_iter().map(|p| p.ty).collect();
                Ok(Type::Function {
                    parameters: param_types,
                    return_type,
                    is_variadic,
                })
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
            Token::Star => {
                self.advance();
                self.skip_newlines();
                if self.is(Token::Fn) {
                    return Err(
                        "functions are code pointers by default; '*Fn' is not allowed, use 'Fn{...}'"
                            .to_string(),
                    );
                }
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
                if !self.is(Token::RParen) {
                    let parameters = self.parse_signature_inner()?;
                    fields = parameters
                        .into_iter()
                        .map(|p| StructFieldType {
                            name: p.name,
                            ty: p.ty,
                        })
                        .collect();
                }
                self.expect(Token::RParen)?;
                Ok(Type::Struct(fields))
            }
            token => Err(format!("expected type, found {:?}", token)),
        }
    }
}

#[cfg(test)]
mod span_tests {
    use super::{Expression, Lexer, Parser, Span, Statement, Type, format_error};

    #[test]
    fn formatter_removes_redundant_file_context() {
        let span = Span {
            file: "sample.zzz".to_string(),
            line: 4,
            col: 2,
        };

        assert_eq!(
            format_error(&span, "in 'sample.zzz': unknown variable 'x'"),
            "sample.zzz:4:2: error: unknown variable 'x'"
        );
    }

    #[test]
    fn lexer_reports_exact_file_line_and_column() {
        let error = Lexer::new_with_file("val x\n  ?".to_string(), "sample.zzz")
            .tokenize()
            .unwrap_err();

        assert_eq!(
            error,
            "sample.zzz:2:3: error: unexpected character '?' at position 8"
        );
    }

    #[test]
    fn parser_keeps_expression_start_span() {
        let (tokens, spans) = Lexer::new_with_file("\n@main 7\n".to_string(), "sample.zzz")
            .tokenize_with_spans()
            .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let (_, definitions, _) = parser.parse_file_declarations("sample.zzz").unwrap();

        let Expression::Spanned { span, .. } = &definitions[0].value else {
            panic!("expected a spanned expression");
        };
        assert_eq!(
            (span.file.as_str(), span.line, span.col),
            ("sample.zzz", 2, 7)
        );
    }

    #[test]
    fn extern_signatures_survive_expression_spans() {
        let (tokens, spans) = Lexer::new_with_file(
            "@extern puts\n(text :: *u8) to s32\n".to_string(),
            "sample.zzz",
        )
        .tokenize_with_spans()
        .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let (externs, _, _) = parser.parse_file_declarations("sample.zzz").unwrap();

        assert_eq!(externs.len(), 1);
        assert_eq!(externs[0].name, "puts");
    }

    #[test]
    fn parser_keeps_statement_start_span() {
        let (tokens, spans) =
            Lexer::new_with_file("\n@main do\n    return 0\ndone\n".to_string(), "sample.zzz")
                .tokenize_with_spans()
                .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let (_, definitions, _) = parser.parse_file_declarations("sample.zzz").unwrap();

        let Expression::Spanned { expression, .. } = &definitions[0].value else {
            panic!("expected a spanned expression");
        };
        let Expression::Block { body, .. } = expression.as_ref() else {
            panic!("expected a block expression");
        };
        let Statement::Spanned { span, .. } = &body[0] else {
            panic!("expected a spanned statement");
        };
        assert_eq!(
            (span.file.as_str(), span.line, span.col),
            ("sample.zzz", 3, 5)
        );
    }

    #[test]
    fn parser_accepts_pointer_sized_integer_types() {
        for (source, expected) in [("usize", Type::USize), ("ssize", Type::SSize)] {
            let (tokens, spans) = Lexer::new_with_file(source.to_string(), "sample.zzz")
                .tokenize_with_spans()
                .unwrap();
            let mut parser = Parser::new_with_spans(tokens, spans);
            assert_eq!(parser.parse_type().unwrap(), expected);
        }
    }

    #[test]
    fn parser_transforms_struct_literal_with_record_suffix() {
        let (tokens, spans) =
            Lexer::new_with_file("(@answer 42) #record".to_string(), "sample.zzz")
                .tokenize_with_spans()
                .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let expression = parser.parse_expression().unwrap();
        let Expression::Spanned { expression, .. } = expression else {
            panic!("expected spanned expression");
        };
        let Expression::Record(fields) = *expression else {
            panic!("expected record expression");
        };

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "answer");
    }

    #[test]
    fn parser_accepts_postfix_pointer_dereference() {
        let (tokens, spans) = Lexer::new_with_file("pointer^".to_string(), "sample.zzz")
            .tokenize_with_spans()
            .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let expression = parser.parse_expression().unwrap();
        let Expression::Spanned { expression, .. } = expression else {
            panic!("expected spanned expression");
        };
        assert!(matches!(*expression, Expression::Dereference(_)));
    }

    #[test]
    fn parser_accepts_keyword_as_member_name() {
        let (tokens, spans) = Lexer::new_with_file("header.val".to_string(), "sample.zzz")
            .tokenize_with_spans()
            .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        let expression = parser.parse_expression().unwrap();
        let Expression::Spanned { expression, .. } = expression else {
            panic!("expected spanned expression");
        };
        assert!(matches!(
            *expression,
            Expression::Member { ref field, .. } if field == "val"
        ));
    }

    #[test]
    fn named_type_fields_require_colon_colon_without_at() {
        let (tokens, spans) = Lexer::new_with_file(
            "@type Record\n*(\n    len :: usize\n)\n".to_string(),
            "sample.zzz",
        )
        .tokenize_with_spans()
        .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        assert!(parser.parse_file_declarations("sample.zzz").is_ok());

        let (tokens, spans) = Lexer::new_with_file(
            "@type Record\n*(\n    @len :: usize\n)\n".to_string(),
            "sample.zzz",
        )
        .tokenize_with_spans()
        .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        assert!(parser.parse_file_declarations("sample.zzz").is_err());
    }

    #[test]
    fn parser_allows_val_as_variable_name() {
        let (tokens, spans) =
            Lexer::new_with_file("header.val -> val\nval[0]\n".to_string(), "sample.zzz")
                .tokenize_with_spans()
                .unwrap();
        let mut parser = Parser::new_with_spans(tokens, spans);
        assert!(parser.parse_statement().is_ok());
        assert!(parser.parse_statement().is_ok());
    }
}
