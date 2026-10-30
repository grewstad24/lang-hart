use std::fmt;

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
pub enum Type {
    Void,
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    Bool,
    Atom(String),
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
pub enum Expression {
    Integer(i64),
    String(Vec<u8>),
    Boolean(bool),
    Atom(String),
    Identifier(String),
    Array(Vec<Expression>),
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
    Decay(Box<Expression>),
    Block {
        signature: Option<Signature>,
        body: Vec<Statement>,
    },
    If {
        arms: Vec<IfArm>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
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
    Return,
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
    position: usize,
}

impl Lexer {
    pub fn new(source: String) -> Self {
        Self {
            chars: source.chars().collect(),
            position: 0,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        while self.position < self.chars.len() {
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
                '^' => {
                    self.position += 1;
                    tokens.push(Token::Caret);
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
        }

        tokens.push(Token::Eof);
        Ok(tokens)
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
            "val" => Token::Val,
            "var" => Token::Var,
            "do" => Token::Do,
            "done" => Token::Done,
            "if" => Token::If,
            "doif" => Token::DoIf,
            "else" => Token::Else,
            "end" => Token::End,
            "return" => Token::Return,
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

    fn is(&self, token: Token) -> bool {
        self.peek() == &token
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

    fn expect_identifier(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            token => Err(format!("expected identifier, found {:?}", token)),
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
    ) -> Result<(Vec<ExternalFunction>, Vec<Definition>), String> {
        let mut extern_functions = Vec::new();
        let mut definitions = Vec::new();

        self.skip_newlines();

        while !self.is(Token::Eof) {
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
                                "in '{}': expected directive after '@' (@val, @extern, @main), found {:?}",
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
                        "extern" => {
                            self.skip_newlines();
                            let name = self.expect_identifier()?;
                            self.skip_newlines();
                            let expr = self.parse_expression()?;
                            self.consume_optional_newline();

                            if let Expression::Signature(signature) = expr {
                                extern_functions.push(ExternalFunction { name, signature });
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

        Ok((extern_functions, definitions))
    }

    pub fn parse_expression(&mut self) -> Result<Expression, String> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_additive()?;

        loop {
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
            let operator = match self.peek() {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Subtract,
                _ => break,
            };

            self.advance();
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
            let operator = match self.peek() {
                Token::Star => BinaryOperator::Multiply,
                Token::Slash => BinaryOperator::Divide,
                Token::Percent => BinaryOperator::Modulo,
                _ => break,
            };

            self.advance();
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
                Ok(Expression::Unary {
                    operator: UnaryOperator::Plus,
                    operand: Box::new(self.parse_unary()?),
                })
            }
            Token::Minus => {
                self.advance();
                Ok(Expression::Unary {
                    operator: UnaryOperator::Minus,
                    operand: Box::new(self.parse_unary()?),
                })
            }
            Token::Ampersand => {
                self.advance();
                Ok(Expression::AddressOf(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_primary()?;

        loop {
            match self.peek() {
                Token::Caret => {
                    self.advance();
                    expression = Expression::Decay(Box::new(expression));
                }
                Token::LParen => {
                    self.advance();
                    let arguments = self.parse_argument_list(Token::RParen)?;
                    expression = Expression::Call {
                        target: Box::new(expression),
                        arguments,
                    };
                }
                Token::Dot => {
                    self.advance();
                    let name = self.expect_identifier()?;
                    expression = Expression::Member {
                        expression: Box::new(expression),
                        field: name,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expression()?;
                    self.expect(Token::RBracket)?;
                    expression = Expression::Index {
                        expression: Box::new(expression),
                        index: Box::new(index),
                    };
                }
                Token::Backtick => {
                    self.advance();
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
                    let ty = self.parse_type()?;
                    expression = Expression::Cast {
                        expression: Box::new(expression),
                        ty,
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
            self.skip_newlines();

            if self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            } else if !self.is(closing_token.clone()) {
                break;
            }
        }

        self.expect(closing_token)?;
        Ok(arguments)
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
            Token::LParen => {
                self.advance();
                if self.is_signature_start() {
                    let sig = self.parse_signature_inner()?;
                    self.expect(Token::RParen)?;
                    let is_variadic = if self.is(Token::PlusPlus) {
                        self.advance();
                        true
                    } else {
                        false
                    };
                    let return_type = if self.is(Token::To) {
                        self.advance();
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
            token => Err(format!("unexpected token in expression: {:?}", token)),
        }
    }

    fn is_signature_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() {
            if self.tokens[idx] == Token::ColonColon {
                return true;
            }
            if self.tokens[idx] == Token::RParen || self.tokens[idx] == Token::Newline {
                break;
            }
            idx += 1;
        }
        false
    }

    fn parse_signature_inner(&mut self) -> Result<Vec<Parameter>, String> {
        let mut parameters = Vec::new();
        while !self.is(Token::RParen) && !self.is(Token::Eof) {
            let name = self.expect_identifier()?;
            self.expect(Token::ColonColon)?;
            let ty = self.parse_type()?;
            parameters.push(Parameter { name, ty });

            if self.is(Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(parameters)
    }

    fn parse_block(&mut self) -> Result<Expression, String> {
        self.expect(Token::Do)?;

        let signature = if self.is(Token::LParen) || self.is(Token::To) {
            let mut parameters = Vec::new();
            if self.is(Token::LParen) {
                self.advance();
                parameters = self.parse_signature_inner()?;
                self.expect(Token::RParen)?;
            }

            let return_type = if self.is(Token::To) {
                self.advance();
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
                let ty = self.parse_type()?;
                self.consume_optional_newline();
                Ok(Statement::Declare { name, ty })
            }
            Token::DoIf => self.parse_doif_statement(),
            Token::Return => {
                self.advance();
                let expr = self.parse_expression()?;
                self.consume_optional_newline();
                Ok(Statement::Return(expr))
            }
            _ => {
                let expression = self.parse_expression()?;
                if self.is(Token::Arrow) {
                    self.advance();
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
        match self.peek().clone() {
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
            Token::TypeBool => {
                self.advance();
                Ok(Type::Bool)
            }
            Token::Star => {
                self.advance();
                let inner = self.parse_type()?;
                Ok(Type::Pointer {
                    element: Box::new(inner),
                })
            }
            Token::LBracket => {
                self.advance();
                if self.is(Token::RBracket) {
                    self.advance();
                    let inner = self.parse_type()?;
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
                    let inner = self.parse_type()?;
                    Ok(Type::Array {
                        element: Box::new(inner),
                        size,
                    })
                }
            }
            Token::LParen => {
                self.advance();
                let mut parameters = Vec::new();
                if !self.is(Token::RParen) {
                    parameters = self.parse_signature_inner()?;
                }
                self.expect(Token::RParen)?;
                let param_types = parameters.into_iter().map(|p| p.ty).collect();
                let is_variadic = if self.is(Token::PlusPlus) {
                    self.advance();
                    true
                } else {
                    false
                };
                let return_type = if self.is(Token::To) {
                    self.advance();
                    Box::new(self.parse_type()?)
                } else {
                    Box::new(Type::Void)
                };
                Ok(Type::Function {
                    parameters: param_types,
                    return_type,
                    is_variadic,
                })
            }
            token => Err(format!("expected type, found {:?}", token)),
        }
    }
}
