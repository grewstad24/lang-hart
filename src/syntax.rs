use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

pub fn format_error(span: &Span, message: &str) -> String {
    let contextual_message = if let Some(stripped) = message.strip_prefix(&format!("{}: error: ", span.file)) {
        stripped
    } else if let Some(stripped) = message.strip_prefix(&format!("in '{}': ", span.file)) {
        stripped
    } else if message.starts_with(&format!("{}:", span.file)) {
        return message.to_string();
    } else {
        message
    };
    format!(
        "{}:{}:{}: error: {}",
        span.file, span.line, span.col, contextual_message
    )
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructFieldType {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldInit {
    pub name: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Opaque,
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
    Symbol(String),
    Alias(String),
    GenericInstance {
        name: String,
        type_arguments: Vec<Type>,
    },
    Struct(Vec<StructFieldType>),
    Tuple(Vec<Type>),
    Inferred,
    Pointer {
        element: Box<Type>,
        is_mut: bool,
    },
    Array {
        element: Box<Type>,
        size: usize,
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

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Opaque => write!(f, "opaque"),
            Type::Never => write!(f, "never"),
            Type::S16 => write!(f, "s16"),
            Type::S32 => write!(f, "s32"),
            Type::S64 => write!(f, "s64"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::USize => write!(f, "usize"),
            Type::SSize => write!(f, "ssize"),
            Type::Symbol(name) => write!(f, ":{}", name),
            Type::Alias(name) => {
                if let Some((base, args)) = name.split_once("__") {
                    let arg_list = args.replace("__", ", ");
                    write!(f, "{}<{}>", base, arg_list)
                } else {
                    write!(f, "{}", name)
                }
            }
            Type::GenericInstance { name, type_arguments } => {
                write!(f, "{}<", name)?;
                for (i, arg) in type_arguments.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
            Type::Struct(fields) => {
                write!(f, "{{ ")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", field.name, field.ty)?;
                }
                write!(f, " }}")
            }
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    write!(f, "()")
                } else if elements.len() == 1 {
                    write!(f, "({},)", elements[0])
                } else {
                    write!(f, "(")?;
                    for (i, elem) in elements.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", elem)?;
                    }
                    write!(f, ")")
                }
            }
            Type::Inferred => write!(f, "_"),
            Type::Pointer { element, is_mut } => {
                if *is_mut {
                    write!(f, "*mut {}", element)
                } else {
                    write!(f, "*{}", element)
                }
            }
            Type::Array { element, size } => write!(f, "[{}]{}", size, element),
            Type::Function { parameters, return_type, is_variadic } => {
                write!(f, "func(")?;
                for (i, param) in parameters.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                if *is_variadic {
                    if !parameters.is_empty() {
                        write!(f, ", ")?;
                    }
                    write!(f, "...")?;
                }
                write!(f, ") -> {}", return_type)
            }
            Type::Union(variants) => {
                for (i, variant) in variants.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", variant)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::Spanned { expression, .. } => write!(f, "{}", expression),
            Expression::Integer(n) => write!(f, "{}", n),
            Expression::String(bytes) => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    write!(f, "\"{}\"", s)
                } else {
                    write!(f, "\"{}\"", String::from_utf8_lossy(bytes))
                }
            }
            Expression::Symbol(name) => write!(f, ":{}", name),
            Expression::Identifier(name) => write!(f, "{}", name),
            Expression::Array(elements) => {
                write!(f, "[")?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "]")
            }
            Expression::Struct(fields) => {
                write!(f, "{{ ")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} := {}", field.name, field.value)?;
                }
                write!(f, " }}")
            }
            Expression::Tuple(elements) => {
                if elements.is_empty() {
                    write!(f, "()")
                } else if elements.len() == 1 {
                    write!(f, "({},)", elements[0])
                } else {
                    write!(f, "(")?;
                    for (i, elem) in elements.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", elem)?;
                    }
                    write!(f, ")")
                }
            }
            Expression::Member { expression, field } => {
                write!(f, "{}.{}", expression, field)
            }
            Expression::Index { expression, index } => {
                write!(f, "{}[{}]", expression, index)
            }
            Expression::AddressOf { expression, is_mut } => {
                if *is_mut {
                    write!(f, "{}.&mut", expression)
                } else {
                    write!(f, "{}.&", expression)
                }
            }
            Expression::Dereference(expression) => {
                write!(f, "{}.*", expression)
            }
            Expression::Cast { expression, ty } => {
                write!(f, "{} as {}", expression, ty)
            }
            Expression::Call { target, arguments } => {
                write!(f, "{}(", target)?;
                for (i, arg) in arguments.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expression::Binary { left, operator, right } => {
                write!(f, "{} {} {}", left, operator, right)
            }
            Expression::Comparison { left, operator, right } => {
                write!(f, "{} {} {}", left, operator, right)
            }
            Expression::Block(_) => write!(f, "{{ ... }}"),
            Expression::Discard(expr) => write!(f, "_{}", expr),
            Expression::Return(expr) => write!(f, "return {}", expr),
            Expression::Yield(expr) => write!(f, "yield {}", expr),
            Expression::Break => write!(f, "break"),
            Expression::Continue => write!(f, "continue"),
            Expression::If { .. } => write!(f, "if ..."),
            Expression::Match { target, .. } => write!(f, "match {}", target),
            Expression::Let { name, value } => write!(f, "{} := {}", name, value),
            Expression::Var { name, ty, value } => {
                write!(f, "mut {}", name)?;
                if let Some(t) = ty {
                    write!(f, ": {}", t)?;
                }
                if let Some(v) = value {
                    write!(f, " := {}", v)?;
                }
                Ok(())
            }
            Expression::Assign { target, value } => write!(f, "{} = {}", target, value),
            Expression::Repeat { count, binder, .. } => {
                if let Some(b) = binder {
                    write!(f, "repeat {} using {}", count, b)
                } else {
                    write!(f, "repeat {}", count)
                }
            }
            Expression::RepeatWhile { condition, .. } => write!(f, "repeat while {}", condition),
            Expression::RepeatUntil { condition, .. } => write!(f, "repeat until {}", condition),
            Expression::For { iterable, binder, .. } => {
                if let Some(b) = binder {
                    write!(f, "repeat for {} using {}", iterable, b)
                } else {
                    write!(f, "repeat for {}", iterable)
                }
            }
            Expression::SizeOf(expr) => write!(f, "@sizeOf {}", expr),
            Expression::Slice(expr) => write!(f, "({} as Slice)", expr),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Add => write!(f, "+"),
            BinaryOperator::Subtract => write!(f, "-"),
            BinaryOperator::Multiply => write!(f, "*"),
            BinaryOperator::Divide => write!(f, "/"),
            BinaryOperator::Modulo => write!(f, "%"),
        }
    }
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonOperator::Equal => write!(f, "="),
            ComparisonOperator::NotEqual => write!(f, "!="),
            ComparisonOperator::Less => write!(f, "<"),
            ComparisonOperator::LessEqual => write!(f, "<="),
            ComparisonOperator::Greater => write!(f, ">"),
            ComparisonOperator::GreaterEqual => write!(f, ">="),
        }
    }
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
    Symbol(String),
    Identifier(String),
    Array(Vec<Expression>),
    Struct(Vec<StructFieldInit>),
    Tuple(Vec<Expression>),
    Member {
        expression: Box<Expression>,
        field: String,
    },
    Index {
        expression: Box<Expression>,
        index: Box<Expression>,
    },
    AddressOf {
        expression: Box<Expression>,
        is_mut: bool,
    },
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
    Yield(Box<Expression>),
    Break,
    Continue,
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
    RepeatWhile {
        condition: Box<Expression>,
        body: Box<Expression>,
    },
    RepeatUntil {
        condition: Box<Expression>,
        body: Box<Expression>,
    },
    For {
        iterable: Box<Expression>,
        binder: Option<String>,
        body: Box<Expression>,
    },
    SizeOf(Box<Expression>),
    Slice(Box<Expression>),
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

pub fn block_contains_yield(expr: &Expression) -> bool {
    match unspanned_expression(expr) {
        Expression::Yield(_) => true,
        Expression::Block(_) => false,
        Expression::If { arms } => arms.iter().any(|arm| {
            if let IfCondition::Expression(e) = &arm.condition {
                block_contains_yield(e) || block_contains_yield(&arm.body)
            } else {
                block_contains_yield(&arm.body)
            }
        }),
        Expression::Match { arms, .. } => arms.iter().any(|arm| block_contains_yield(&arm.body)),
        Expression::Repeat { body, .. }
        | Expression::RepeatWhile { body, .. }
        | Expression::RepeatUntil { body, .. }
        | Expression::For { body, .. } => block_contains_yield(body),
        Expression::Discard(e) => block_contains_yield(e),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub signature: Option<Signature>,
    pub value: Expression,
    pub is_def: bool,
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
    pub is_meta: bool,
    pub type_parameters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub ty: Type,
    pub initializer: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRule {
    pub symbols: Vec<String>,
    pub target: NamespacePath,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleDecl {
    Declaration {
        name: String,
        exports: Vec<ExportRule>,
        imports: Vec<NamespacePath>,
        span: Span,
    },
    Member {
        name: String,
        span: Span,
    },
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
    pub name: Option<String>,
    pub dir_path: String,
    pub source_files: Vec<SourceFile>,
    pub exports: Vec<ExportRule>,
    pub imports: Vec<NamespacePath>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFile {
    pub module_decl: Option<ModuleDecl>,
    pub exports: Vec<ExportRule>,
    pub imports: Vec<NamespacePath>,
    pub source_file: SourceFile,
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
    Symbol(String),
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
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
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
    Def,
    TypeKeyword,
    Const,
    If,
    Else,
    Match,
    Switch,
    Using,
    Return,
    Yield,
    Break,
    Continue,
    As,
    Repeat,
    While,
    Until,
    For,
    Mut,
    Decl,
    Mod,
    Export,
    Import,
    To,
    TypeOpaque,
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
    Newline,
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Identifier(s) => write!(f, "'{}'", s),
            Token::Integer(n) => write!(f, "{}", n),
            Token::Float(fl) => write!(f, "{}", fl),
            Token::String(bytes) => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    write!(f, "\"{}\"", s)
                } else {
                    write!(f, "string literal")
                }
            }
            Token::Symbol(s) => write!(f, ":{}", s),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
            Token::LBracket => write!(f, "'['"),
            Token::RBracket => write!(f, "']'"),
            Token::LBrace => write!(f, "'{{'"),
            Token::RBrace => write!(f, "'}}'"),
            Token::Colon => write!(f, "':'"),
            Token::ColonColon => write!(f, "'::'"),
            Token::ColonEqual => write!(f, "':='"),
            Token::PlusEqual => write!(f, "'+='"),
            Token::MinusEqual => write!(f, "'-='"),
            Token::StarEqual => write!(f, "'*='"),
            Token::SlashEqual => write!(f, "'/='"),
            Token::Comma => write!(f, "','"),
            Token::Dot => write!(f, "'.'"),
            Token::Plus => write!(f, "'+'"),
            Token::Minus => write!(f, "'-'"),
            Token::Star => write!(f, "'*'"),
            Token::Slash => write!(f, "'/'"),
            Token::Percent => write!(f, "'%'"),
            Token::Ampersand => write!(f, "'&'"),
            Token::Caret => write!(f, "'^'"),
            Token::Pipe => write!(f, "'|'"),
            Token::At => write!(f, "'@'"),
            Token::Equal => write!(f, "'='"),
            Token::NotEqual => write!(f, "'!='"),
            Token::Less => write!(f, "'<'"),
            Token::LessEqual => write!(f, "'<='"),
            Token::Greater => write!(f, "'>'"),
            Token::GreaterEqual => write!(f, "'>='"),
            Token::FatArrow => write!(f, "'=>'"),
            Token::Arrow => write!(f, "'->'"),
            Token::DotDot => write!(f, "'..'"),
            Token::DotDotDot => write!(f, "'...'"),
            Token::Underscore => write!(f, "'_'"),
            Token::FnKeyword => write!(f, "'func'"),
            Token::Def => write!(f, "'def'"),
            Token::TypeKeyword => write!(f, "'type'"),
            Token::Const => write!(f, "'const'"),
            Token::If => write!(f, "'if'"),
            Token::Else => write!(f, "'else'"),
            Token::Match => write!(f, "'match'"),
            Token::Switch => write!(f, "'switch'"),
            Token::Using => write!(f, "'using'"),
            Token::Return => write!(f, "'return'"),
            Token::Yield => write!(f, "'yield'"),
            Token::Break => write!(f, "'break'"),
            Token::Continue => write!(f, "'continue'"),
            Token::As => write!(f, "'as'"),
            Token::Repeat => write!(f, "'repeat'"),
            Token::While => write!(f, "'while'"),
            Token::Until => write!(f, "'until'"),
            Token::For => write!(f, "'for'"),
            Token::Mut => write!(f, "'mut'"),
            Token::Decl => write!(f, "'decl'"),
            Token::Mod => write!(f, "'mod'"),
            Token::Export => write!(f, "'export'"),
            Token::Import => write!(f, "'import'"),
            Token::To => write!(f, "'to'"),
            Token::Extern => write!(f, "'extern'"),
            Token::TypeOpaque => write!(f, "'opaque'"),
            Token::Never => write!(f, "'never'"),
            Token::TypeS16 => write!(f, "'s16'"),
            Token::TypeS32 => write!(f, "'s32'"),
            Token::TypeS64 => write!(f, "'s64'"),
            Token::TypeU8 => write!(f, "'u8'"),
            Token::TypeU16 => write!(f, "'u16'"),
            Token::TypeU32 => write!(f, "'u32'"),
            Token::TypeU64 => write!(f, "'u64'"),
            Token::TypeUSize => write!(f, "'usize'"),
            Token::TypeSSize => write!(f, "'ssize'"),
            Token::Newline => write!(f, "newline"),
            Token::Eof => write!(f, "end of file"),
        }
    }
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
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::PlusEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Plus);
                    }
                }
                '-' => {
                    if self.peek_char(1) == Some('>') {
                        self.position += 2;
                        tokens.push(Token::Arrow);
                    } else if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::MinusEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Minus);
                    }
                }
                '*' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::StarEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Star);
                    }
                }
                '/' => {
                    if self.peek_char(1) == Some('=') {
                        self.position += 2;
                        tokens.push(Token::SlashEqual);
                    } else {
                        self.position += 1;
                        tokens.push(Token::Slash);
                    }
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
                    } else {
                        let is_atom = if let Some(c) = self.peek_char(1) {
                            if c.is_alphabetic() || c == '_' {
                                let prev_is_identifier = tokens.last().map_or(false, |tok| {
                                    if tokens.len() >= 2 && matches!(tokens[tokens.len() - 2], Token::At) {
                                        return false;
                                    }
                                    matches!(
                                        tok,
                                        Token::Identifier(_)
                                            | Token::Const
                                            | Token::TypeKeyword
                                            | Token::RParen
                                            | Token::RBracket
                                    )
                                });
                                !prev_is_identifier
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if is_atom {
                            tokens.push(self.lex_atom()?);
                        } else {
                            self.position += 1;
                            tokens.push(Token::Colon);
                        }
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
            return Err("symbol identifier cannot be empty".to_string());
        }

        Ok(Token::Symbol(name))
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
            "const" => Token::Const,
            "type" => Token::TypeKeyword,
            "extern" => Token::Extern,
            "func" => Token::FnKeyword,
            "def" => Token::Def,
            "if" => Token::If,
            "else" => Token::Else,
            "match" => Token::Match,
            "switch" => Token::Switch,
            "using" => Token::Using,
            "return" => Token::Return,
            "yield" => Token::Yield,
            "break" => Token::Break,
            "continue" => Token::Continue,
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
            "repeat" => Token::Repeat,
            "while" => Token::While,
            "until" => Token::Until,
            "for" => Token::For,
            "mut" => Token::Mut,
            "decl" => Token::Decl,
            "mod" => Token::Mod,
            "export" => Token::Export,
            "import" => Token::Import,
            "to" => Token::To,
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
                        '0' => {
                            bytes.push(0);
                            self.position += 1;
                        }
                        'n' => {
                            bytes.push(b'\n');
                            self.position += 1;
                        }
                        'r' => {
                            bytes.push(b'\r');
                            self.position += 1;
                        }
                        't' => {
                            bytes.push(b'\t');
                            self.position += 1;
                        }
                        '\\' => {
                            bytes.push(b'\\');
                            self.position += 1;
                        }
                        '"' => {
                            bytes.push(b'"');
                            self.position += 1;
                        }
                        'u' => {
                            self.position += 1;
                            if self.position >= self.chars.len() || self.chars[self.position] != '{' {
                                return Err("expected '{' after '\\u'".to_string());
                            }
                            self.position += 1;

                            let start = self.position;
                            while self.position < self.chars.len()
                                && self.chars[self.position] != '}'
                                && self.chars[self.position] != '"'
                                && self.chars[self.position] != '\n'
                            {
                                self.position += 1;
                            }

                            if self.position >= self.chars.len() || self.chars[self.position] != '}' {
                                return Err("unterminated unicode escape sequence, expected '}'".to_string());
                            }

                            let hex_str: String = self.chars[start..self.position].iter().collect();
                            self.position += 1; // consume '}'

                            if hex_str.is_empty() {
                                return Err("empty unicode escape sequence '\\u{}'".to_string());
                            }
                            if hex_str.len() > 6 {
                                return Err(format!("unicode escape sequence too long: '\\u{{{}}}'", hex_str));
                            }

                            let code_point = u32::from_str_radix(&hex_str, 16)
                                .map_err(|_| format!("invalid hex in unicode escape sequence: '\\u{{{}}}'", hex_str))?;

                            let ch = char::from_u32(code_point)
                                .ok_or_else(|| format!("invalid unicode code point: '\\u{{{}}}'", hex_str))?;

                            let mut buf = [0u8; 4];
                            let encoded = ch.encode_utf8(&mut buf);
                            bytes.extend_from_slice(encoded.as_bytes());
                        }
                        other => {
                            return Err(format!("unknown string escape '\\{}'", other));
                        }
                    }
                }
                '\n' => {
                    return Err("unterminated string literal".to_string());
                }
                other => {
                    let mut buf = [0u8; 4];
                    let encoded = other.encode_utf8(&mut buf);
                    bytes.extend_from_slice(encoded.as_bytes());
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
    switch_counter: usize,
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
            switch_counter: 0,
        }
    }

    pub fn new_with_spans(tokens: Vec<Token>, spans: Vec<Span>) -> Self {
        debug_assert_eq!(tokens.len(), spans.len());
        Self {
            tokens,
            spans,
            cursor: 0,
            switch_counter: 0,
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
            Err(format!("expected {}, found {}", expected, current))
        }
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            Token::Const => {
                self.advance();
                Ok("const".to_string())
            }
            Token::Mut => {
                self.advance();
                Ok("mut".to_string())
            }
            token => Err(format!("expected identifier, found {}", token)),
        }
    }

    fn expect_field_name(&mut self) -> Result<String, String> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            Token::Const => {
                self.advance();
                Ok("const".to_string())
            }
            Token::Mut => {
                self.advance();
                Ok("mut".to_string())
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
            Token::Switch => {
                self.advance();
                Ok("switch".to_string())
            }
            Token::Using => {
                self.advance();
                Ok("using".to_string())
            }
            Token::Return => {
                self.advance();
                Ok("return".to_string())
            }
            Token::Yield => {
                self.advance();
                Ok("yield".to_string())
            }
            Token::Break => {
                self.advance();
                Ok("break".to_string())
            }
            Token::Continue => {
                self.advance();
                Ok("continue".to_string())
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
            Token::Repeat => {
                self.advance();
                Ok("repeat".to_string())
            }
            Token::While => {
                self.advance();
                Ok("while".to_string())
            }
            Token::Until => {
                self.advance();
                Ok("until".to_string())
            }
            Token::For => {
                self.advance();
                Ok("for".to_string())
            }
            Token::Decl => {
                self.advance();
                Ok("decl".to_string())
            }
            Token::Mod => {
                self.advance();
                Ok("mod".to_string())
            }
            Token::Export => {
                self.advance();
                Ok("export".to_string())
            }
            Token::Import => {
                self.advance();
                Ok("import".to_string())
            }
            Token::To => {
                self.advance();
                Ok("to".to_string())
            }
            Token::TypeOpaque => {
                self.advance();
                Ok("opaque".to_string())
            }
            Token::TypeKeyword => {
                self.advance();
                Ok("type".to_string())
            }
            token => Err(format!("expected field name identifier, found {}", token)),
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
        self.parse_block_body()
    }

    pub fn parse_block_body(&mut self) -> Result<Expression, String> {
        let mut expressions = Vec::new();

        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
            let expr = self.parse_expression()?;

            if self.is(Token::RBrace) {
                // No trailing newline before '}' -> automatically return
                let ret_expr = match unspanned_expression(&expr) {
                    Expression::Return(_) | Expression::Yield(_) | Expression::Break | Expression::Continue => expr,
                    _ => Expression::Return(Box::new(expr)),
                };
                expressions.push(ret_expr);
                break;
            } else if self.is(Token::Newline) {
                // Trailing newline present -> evaluate, discard value, continue
                self.skip_newlines();
                match unspanned_expression(&expr) {
                    Expression::Return(_) | Expression::Yield(_) | Expression::Break | Expression::Continue => {
                        expressions.push(expr);
                    }
                    _ if block_contains_yield(&expr) => {
                        expressions.push(expr);
                    }
                    _ => {
                        expressions.push(Expression::Discard(Box::new(expr)));
                    }
                }
            } else {
                return Err(format!(
                    "expected newline or '}}' after expression, found {}",
                    self.peek()
                ));
            }
        }

        self.expect(Token::RBrace)?;

        Ok(Expression::Block(expressions))
    }

    pub fn parse_namespace_path(&mut self) -> Result<NamespacePath, String> {
        self.skip_newlines();
        if !self.is(Token::Dot) {
            return Err(format!(
                "expected namespace starting with '.', found {}",
                self.peek()
            ));
        }
        self.advance(); // consume '.'
        let mut segments = Vec::new();
        if self.is_identifier_like() {
            segments.push(self.expect_field_name()?);
            while self.is(Token::Dot) && self.is_identifier_like_at(self.cursor + 1) {
                self.advance(); // consume '.'
                segments.push(self.expect_field_name()?);
            }
        }
        Ok(NamespacePath::from_segments(segments))
    }

    pub fn parse_export_directive(&mut self) -> Result<Vec<ExportRule>, String> {
        let span = self.current_span().clone();
        self.expect(Token::Export)?;
        self.skip_newlines();

        let mut rules = Vec::new();

        self.expect(Token::LBrace)?;
        self.skip_newlines();

        if self.is(Token::RBrace) {
            // export { }
            self.advance();
            self.consume_optional_newline();
            return Ok(rules);
        }

        if self.is(Token::LBrace) {
            // Multi-group export:
            // export {
            //       { print, println } to .print,
            //       { printf } to .print.raw_printf
            // }
            while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                self.skip_newlines();
                if self.is(Token::RBrace) {
                    break;
                }
                let item_span = self.current_span().clone();
                self.expect(Token::LBrace)?;
                self.skip_newlines();
                let mut symbols = Vec::new();
                while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                    symbols.push(self.expect_field_name()?);
                    self.skip_newlines();
                    while self.is(Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(Token::RBrace)?;
                self.skip_newlines();
                let target = if self.is(Token::To) {
                    self.advance();
                    self.skip_newlines();
                    self.parse_namespace_path()?
                } else {
                    NamespacePath::root()
                };
                rules.push(ExportRule {
                    symbols,
                    target,
                    span: item_span,
                });
                self.skip_newlines();
                while self.is(Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.expect(Token::RBrace)?;
        } else {
            // Single-group export:
            // export { add, sub, mul, div } to .math
            // export { String, Integer, Record, Generic } to .
            let mut symbols = Vec::new();
            while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                symbols.push(self.expect_field_name()?);
                self.skip_newlines();
                while self.is(Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.expect(Token::RBrace)?;
            self.skip_newlines();
            let target = if self.is(Token::To) {
                self.advance();
                self.skip_newlines();
                self.parse_namespace_path()?
            } else {
                NamespacePath::root()
            };
            rules.push(ExportRule {
                symbols,
                target,
                span,
            });
        }

        self.consume_optional_newline();
        Ok(rules)
    }

    pub fn parse_export_rule(&mut self) -> Result<ExportRule, String> {
        let rules = self.parse_export_directive()?;
        rules
            .into_iter()
            .next()
            .ok_or_else(|| "empty export directive".to_string())
    }

    pub fn parse_import_directive(&mut self) -> Result<Vec<NamespacePath>, String> {
        self.expect(Token::Import)?;
        self.skip_newlines();
        let mut imports = Vec::new();
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
            imports.push(self.parse_namespace_path()?);
            self.skip_newlines();
            while self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(Token::RBrace)?;
        self.consume_optional_newline();
        Ok(imports)
    }

    pub fn parse_file_declarations(&mut self, file_path: &str) -> Result<ParsedFile, String> {
        let mut module_decl: Option<ModuleDecl> = None;
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        let mut extern_functions = Vec::new();
        let mut definitions = Vec::new();
        let mut type_aliases = Vec::new();
        let mut has_top_level_export = false;
        let mut has_top_level_import = false;

        self.skip_newlines();

        while !self.is(Token::Eof) {
            let declaration_span = self.current_span().clone();
            match self.peek() {
                Token::Decl => {
                    if module_decl.is_some() {
                        return Err(format!(
                            "{}: error: file cannot declare more than one module declaration",
                            file_path
                        ));
                    }
                    if !exports.is_empty() || !imports.is_empty() {
                        return Err(format!(
                            "{}: error: exports and imports must be declared inside the 'decl mod' block",
                            file_path
                        ));
                    }
                    self.advance(); // consume 'decl'
                    self.skip_newlines();
                    self.expect(Token::Mod)?;
                    self.skip_newlines();
                    let name = self.expect_field_name()?;
                    self.skip_newlines();
                    self.expect(Token::LBrace)?;
                    self.skip_newlines();
                    let mut decl_exports = Vec::new();
                    let mut decl_imports = Vec::new();
                    let mut has_export = false;
                    let mut has_import = false;
                    while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                        self.skip_newlines();
                        if self.is(Token::RBrace) {
                            break;
                        }
                        match self.peek() {
                            Token::Export => {
                                if has_export {
                                    return Err(format!(
                                        "{}: error: module '{}' cannot have more than one export statement",
                                        file_path, name
                                    ));
                                }
                                has_export = true;
                                let mut rules = self.parse_export_directive()?;
                                decl_exports.append(&mut rules);
                            }
                            Token::Import => {
                                if has_import {
                                    return Err(format!(
                                        "{}: error: module '{}' cannot have more than one import statement",
                                        file_path, name
                                    ));
                                }
                                has_import = true;
                                let mut imps = self.parse_import_directive()?;
                                decl_imports.append(&mut imps);
                            }
                            Token::Newline => {
                                self.skip_newlines();
                            }
                            Token::Comma => {
                                self.advance();
                                self.skip_newlines();
                            }
                            token => {
                                return Err(format!(
                                    "{}: error: unexpected token inside 'decl mod {}': {}. Only 'export' and 'import' are allowed.",
                                    file_path, name, token
                                ));
                            }
                        }
                        self.skip_newlines();
                    }
                    self.expect(Token::RBrace)?;
                    self.consume_optional_newline();
                    module_decl = Some(ModuleDecl::Declaration {
                        name,
                        exports: decl_exports,
                        imports: decl_imports,
                        span: declaration_span,
                    });
                }
                Token::Mod => {
                    if module_decl.is_some() {
                        return Err(format!(
                            "{}: error: file cannot declare more than one module declaration",
                            file_path
                        ));
                    }
                    if !exports.is_empty() || !imports.is_empty() {
                        return Err(format!(
                            "{}: error: only the module declaration site ('decl mod') can declare imports and exports; member files cannot",
                            file_path
                        ));
                    }
                    self.advance(); // consume 'mod'
                    self.skip_newlines();
                    let name = self.expect_field_name()?;
                    self.consume_optional_newline();
                    module_decl = Some(ModuleDecl::Member {
                        name,
                        span: declaration_span,
                    });
                }
                Token::Export => {
                    if let Some(ModuleDecl::Member { name, .. }) = &module_decl {
                        return Err(format!(
                            "{}: error: member file of module '{}' cannot declare exports. Only 'decl mod {}' can declare exports.",
                            file_path, name, name
                        ));
                    }
                    if let Some(ModuleDecl::Declaration { name, .. }) = &module_decl {
                        return Err(format!(
                            "{}: error: exports for module '{}' must be declared inside the 'decl mod {} {{ ... }}' block",
                            file_path, name, name
                        ));
                    }
                    if has_top_level_export {
                        return Err(format!(
                            "{}: error: cannot have more than one export statement per module",
                            file_path
                        ));
                    }
                    has_top_level_export = true;
                    let mut rules = self.parse_export_directive()?;
                    exports.append(&mut rules);
                }
                Token::Import => {
                    if let Some(ModuleDecl::Member { name, .. }) = &module_decl {
                        return Err(format!(
                            "{}: error: member file of module '{}' cannot declare imports. Only 'decl mod {}' can declare imports.",
                            file_path, name, name
                        ));
                    }
                    if let Some(ModuleDecl::Declaration { name, .. }) = &module_decl {
                        return Err(format!(
                            "{}: error: imports for module '{}' must be declared inside the 'decl mod {} {{ ... }}' block",
                            file_path, name, name
                        ));
                    }
                    if has_top_level_import {
                        return Err(format!(
                            "{}: error: cannot have more than one import statement per module",
                            file_path
                        ));
                    }
                    has_top_level_import = true;
                    let mut imps = self.parse_import_directive()?;
                    imports.append(&mut imps);
                }
                Token::FnKeyword => {
                    let def = self.parse_fn_definition()?;
                    definitions.push(def);
                }
                Token::Def => {
                    let def = self.parse_def_definition()?;
                    definitions.push(def);
                }
                Token::TypeKeyword => {
                    self.advance(); // consume 'type'
                    self.skip_newlines();
                    let name = self.expect_identifier()?;
                    self.skip_newlines();
                    let mut type_parameters = Vec::new();
                    if self.is(Token::Less) {
                        self.advance();
                        self.skip_newlines();
                        while !self.is(Token::Greater) && !self.is(Token::Eof) {
                            type_parameters.push(self.expect_identifier()?);
                            self.skip_newlines();
                            if self.is(Token::Comma) {
                                self.advance();
                                self.skip_newlines();
                            }
                        }
                        self.expect(Token::Greater)?;
                        self.skip_newlines();
                    }
                    self.expect(Token::ColonEqual)?;
                    self.skip_newlines();
                    let target = self.parse_type()?;
                    self.consume_optional_newline();
                    let is_meta = !type_parameters.is_empty();
                    type_aliases.push(TypeAlias {
                        name,
                        target,
                        span: declaration_span,
                        is_meta,
                        type_parameters,
                    });
                }
                Token::Const => {
                    self.advance(); // consume 'const'
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
                        is_def: false,
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
                        if self.is(Token::Colon) || self.is(Token::ColonColon) {
                            self.advance();
                        } else {
                            return Err(format!(
                                "expected ':' after parameter name, found {}",
                                self.peek()
                            ));
                        }
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
                    return Err(format_error(
                        self.current_span(),
                        &format!(
                            "unexpected token at top level: {}. Declarations must start with 'func', 'def', 'type', 'const', 'extern', 'decl mod', 'mod', 'export', or 'import'",
                            token
                        ),
                    ));
                }
            }

            self.skip_newlines();
        }

        if let Some(ModuleDecl::Declaration {
            exports: ref decl_exports,
            imports: ref mut decl_imports,
            ..
        }) = module_decl
        {
            if !decl_imports.contains(&NamespacePath::root()) {
                decl_imports.insert(0, NamespacePath::root());
            }
            exports = decl_exports.clone();
            imports = decl_imports.clone();
        } else {
            if !imports.contains(&NamespacePath::root()) {
                imports.insert(0, NamespacePath::root());
            }
        }

        let source_file = SourceFile {
            file_path: file_path.to_string(),
            extern_functions,
            definitions,
            type_aliases,
        };

        Ok(ParsedFile {
            module_decl,
            exports,
            imports,
            source_file,
        })
    }

    fn parse_fn_definition(&mut self) -> Result<Definition, String> {
        self.advance(); // consume 'func'
        let name = self.expect_identifier()?;

        self.expect(Token::LParen)?;
        self.skip_newlines();
        let mut parameters = Vec::new();
        while !self.is(Token::RParen) && !self.is(Token::Eof) {
            let param_name = self.expect_field_name()?;
            if self.is(Token::Colon) || self.is(Token::ColonColon) {
                self.advance();
            } else {
                return Err(format!(
                    "expected ':' after parameter name, found {}",
                    self.peek()
                ));
            }
            self.skip_newlines();
            let ty = self.parse_type()?;
            parameters.push(Parameter {
                name: param_name,
                ty,
            });
            self.skip_newlines();
            while self.is(Token::Comma) {
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
            is_def: false,
        })
    }

    pub fn parse_def_definition(&mut self) -> Result<Definition, String> {
        self.expect(Token::Def)?;
        let name = self.expect_identifier()?;
        if name == "main" {
            return Err(format_error(
                self.current_span(),
                "entry point 'main' must be a function ('func'), never a 'def'",
            ));
        }

        self.expect(Token::LParen)?;
        self.skip_newlines();
        let mut parameters = Vec::new();
        while !self.is(Token::RParen) && !self.is(Token::Eof) {
            let param_name = self.expect_field_name()?;
            self.skip_newlines();
            let ty = if self.is(Token::Colon) || self.is(Token::ColonColon) {
                self.advance();
                self.skip_newlines();
                self.parse_type()?
            } else {
                Type::Inferred
            };
            parameters.push(Parameter {
                name: param_name,
                ty,
            });
            self.skip_newlines();
            while self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(Token::RParen)?;

        self.skip_newlines();
        let return_type = if self.is(Token::Arrow) {
            self.advance();
            self.skip_newlines();
            self.parse_type()?
        } else {
            Type::Inferred
        };

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
            is_def: true,
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

        if let Some(op) = match self.peek() {
            Token::PlusEqual => Some(BinaryOperator::Add),
            Token::MinusEqual => Some(BinaryOperator::Subtract),
            Token::StarEqual => Some(BinaryOperator::Multiply),
            Token::SlashEqual => Some(BinaryOperator::Divide),
            _ => None,
        } {
            self.advance();
            self.skip_newlines();
            let value = self.parse_assignment()?;
            return Ok(Expression::Assign {
                target: Box::new(expression.clone()),
                value: Box::new(Expression::Binary {
                    left: Box::new(expression),
                    operator: op,
                    right: Box::new(value),
                }),
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
                    Token::Slash | Token::Percent => {
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
        let expr = self.parse_primary()?;
        self.parse_postfix(expr)
    }

    fn parse_postfix(&mut self, mut expression: Expression) -> Result<Expression, String> {
        loop {
            if self.is(Token::Newline) {
                match self.peek_non_newline() {
                    Token::Dot | Token::As => {
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
                        let is_mut = if self.is(Token::Mut) {
                            self.advance();
                            true
                        } else {
                            false
                        };
                        expression = Expression::AddressOf {
                            expression: Box::new(expression),
                            is_mut,
                        };
                    } else if let Token::Integer(value) = self.peek() {
                        let field = value.to_string();
                        self.advance();
                        expression = Expression::Member {
                            expression: Box::new(expression),
                            field,
                        };
                    } else {
                        let field = self.expect_field_name()?;
                        expression = Expression::Member {
                            expression: Box::new(expression),
                            field,
                        };
                    }
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

    pub fn is_identifier_like_at(&self, idx: usize) -> bool {
        if idx >= self.tokens.len() {
            return false;
        }
        matches!(
            &self.tokens[idx],
            Token::Identifier(_)
                | Token::Const
                | Token::Mut
                | Token::If
                | Token::Else
                | Token::Match
                | Token::Switch
                | Token::Using
                | Token::Return
                | Token::Yield
                | Token::Break
                | Token::Continue
                | Token::As
                | Token::Repeat
                | Token::While
                | Token::Until
                | Token::For
                | Token::Decl
                | Token::Mod
                | Token::Export
                | Token::Import
                | Token::To
                | Token::TypeOpaque
                | Token::TypeKeyword
                | Token::TypeS16
                | Token::TypeS32
                | Token::TypeS64
                | Token::TypeU8
                | Token::TypeU16
                | Token::TypeU32
                | Token::TypeU64
                | Token::TypeUSize
                | Token::TypeSSize
        )
    }

    pub fn is_identifier_like(&self) -> bool {
        self.is_identifier_like_at(self.cursor)
    }

    fn is_struct_literal_start(&self) -> bool {
        let mut idx = self.cursor;
        while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
            idx += 1;
        }
        if idx < self.tokens.len() {
            match &self.tokens[idx] {
                Token::Identifier(_)
                | Token::Const
                | Token::Mut
                | Token::If
                | Token::Else
                | Token::Match
                | Token::Switch
                | Token::Using
                | Token::Return
                | Token::Yield
                | Token::Break
                | Token::Continue
                | Token::As
                | Token::Repeat
                | Token::While
                | Token::Until
                | Token::For
                | Token::Decl
                | Token::Mod
                | Token::Export
                | Token::Import
                | Token::To
                | Token::TypeOpaque
                | Token::TypeKeyword
                | Token::TypeS16
                | Token::TypeS32
                | Token::TypeS64
                | Token::TypeU8
                | Token::TypeU16
                | Token::TypeU32
                | Token::TypeU64
                | Token::TypeUSize
                | Token::TypeSSize => {
                    idx += 1;
                    while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
                        idx += 1;
                    }
                    return idx < self.tokens.len() && self.tokens[idx] == Token::Colon;
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
            Token::String(value) => {
                self.advance();
                Ok(Expression::String(value))
            }
            Token::Symbol(name) => {
                self.advance();
                Ok(Expression::Symbol(name))
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Expression::Identifier(name))
            }
            Token::At => {
                self.advance();
                let tag = self.expect_identifier()?;
                self.skip_newlines();
                if tag == "sizeOf" {
                    let operand = self.parse_unary()?;
                    return Ok(Expression::SizeOf(Box::new(operand)));
                }
                return Err(format!(
                    "unknown tag '@{}', expected '@sizeOf'",
                    tag
                ));
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();

                if self.is(Token::RParen) {
                    self.advance();
                    Ok(Expression::Tuple(Vec::new()))
                } else {
                    let first = self.parse_expression()?;
                    self.skip_newlines();
                    if self.is(Token::Comma) {
                        let mut elements = vec![first];
                        while self.is(Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                            if self.is(Token::RParen) {
                                break;
                            }
                            elements.push(self.parse_expression()?);
                            self.skip_newlines();
                        }
                        self.expect(Token::RParen)?;
                        Ok(Expression::Tuple(elements))
                    } else {
                        self.expect(Token::RParen)?;
                        Ok(first)
                    }
                }
            }
            Token::LBracket => {
                self.advance();
                let elements = self.parse_argument_list(Token::RBracket)?;
                Ok(Expression::Array(elements))
            }
            Token::TypeKeyword
            | Token::TypeS16
            | Token::TypeS32
            | Token::TypeS64
            | Token::TypeU8
            | Token::TypeU16
            | Token::TypeU32
            | Token::TypeU64
            | Token::TypeUSize
            | Token::TypeSSize
            | Token::TypeOpaque => {
                let tok = self.peek().clone();
                return Err(format!(
                    "unexpected type token '{}' in expression position",
                    tok
                ));
            }
            Token::LBrace => {
                self.advance();
                self.skip_newlines();

                if self.is_struct_literal_start() {
                    let fields = self.parse_struct_fields(Token::RBrace)?;
                    Ok(Expression::Struct(fields))
                } else {
                    self.parse_block_body()
                }
            }
            Token::If => self.parse_if_expression(),
            Token::Switch => self.parse_switch_expression(),
            Token::Match => self.parse_match_expression(),
            Token::Const => {
                self.advance();
                let name = self.expect_identifier()?;
                if self.is(Token::Colon) || self.is(Token::ColonColon) {
                    return Err(format_error(
                        self.current_span(),
                        &format!(
                            "constants cannot be declared without an initialiser: 'const {} : ...' is prohibited, use 'const {} := <value>'",
                            name, name
                        ),
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
            Token::Mut => {
                self.advance();
                let name = self.expect_identifier()?;
                if self.is(Token::Colon) || self.is(Token::ColonColon) {
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
                    Err(format_error(
                        self.current_span(),
                        &format!(
                            "expected ':' or ':=' after 'mut {}', found {}",
                            name,
                            self.peek()
                        ),
                    ))
                }
            }
            Token::Return => {
                self.advance();
                self.skip_newlines();
                let expr = self.parse_expression()?;
                Ok(Expression::Return(Box::new(expr)))
            }
            Token::Yield => {
                self.advance();
                if self.is(Token::Newline) || self.is(Token::RBrace) {
                    Ok(Expression::Yield(Box::new(Expression::Symbol("none".to_string()))))
                } else {
                    self.skip_newlines();
                    let expr = self.parse_expression()?;
                    Ok(Expression::Yield(Box::new(expr)))
                }
            }
            Token::Break => {
                self.advance();
                Ok(Expression::Break)
            }
            Token::Continue => {
                self.advance();
                Ok(Expression::Continue)
            }
            Token::Repeat => {
                self.advance();
                self.skip_newlines();
                if self.is(Token::While) {
                    self.advance();
                    self.skip_newlines();
                    let condition = self.parse_expression()?;
                    self.skip_newlines();
                    let body = self.parse_block()?;
                    Ok(Expression::RepeatWhile {
                        condition: Box::new(condition),
                        body: Box::new(body),
                    })
                } else if self.is(Token::Until) {
                    self.advance();
                    self.skip_newlines();
                    let condition = self.parse_expression()?;
                    self.skip_newlines();
                    let body = self.parse_block()?;
                    Ok(Expression::RepeatUntil {
                        condition: Box::new(condition),
                        body: Box::new(body),
                    })
                } else if self.is(Token::For) {
                    self.parse_for_expression()
                } else {
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
                    let body = self.parse_block()?;
                    Ok(Expression::Repeat {
                        count: Box::new(count),
                        binder,
                        body: Box::new(body),
                    })
                }
            }
            token => Err(format!("unexpected token in expression: {}", token)),
        }
    }

    pub fn parse_struct_fields(
        &mut self,
        closing_token: Token,
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
            self.expect(Token::Colon)?;
            self.skip_newlines();

            let value = self.parse_expression()?;

            fields.push(StructFieldInit { name, value });

            self.skip_newlines();
            while self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
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

        let binding_name = if self.is(Token::Using) {
            self.advance();
            self.skip_newlines();
            let name = self.expect_identifier()?;
            self.skip_newlines();
            name
        } else {
            "_".to_string()
        };

        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut arms = Vec::new();

        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
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
            while self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }

        self.expect(Token::RBrace)?;

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

            while self.is(Token::Comma) {
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

        if !has_else {
            arms.push(IfArm {
                condition: IfCondition::Else,
                body: Box::new(Expression::Symbol("none".to_string())),
            });
        }

        Ok(Expression::If { arms })
    }

    fn parse_switch_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::Switch)?;
        self.skip_newlines();

        let target = self.parse_expression()?;
        self.skip_newlines();

        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let (target_expr, let_binding) = match unspanned_expression(&target) {
            Expression::Identifier(_) => (target, None),
            _ => {
                self.switch_counter += 1;
                let var_name = format!("__switch_{}", self.switch_counter);
                let let_expr = Expression::Let {
                    name: var_name.clone(),
                    value: Box::new(target),
                };
                (Expression::Identifier(var_name), Some(let_expr))
            }
        };

        let mut arms = Vec::new();
        let mut has_else = false;

        while !self.is(Token::RBrace) && !self.is(Token::Eof) {
            self.skip_newlines();
            if self.is(Token::RBrace) {
                break;
            }

            let condition = if self.is(Token::Else) {
                self.advance();
                has_else = true;
                IfCondition::Else
            } else {
                let case_expr = self.parse_expression()?;
                IfCondition::Expression(Expression::Comparison {
                    left: Box::new(target_expr.clone()),
                    operator: ComparisonOperator::Equal,
                    right: Box::new(case_expr),
                })
            };

            self.skip_newlines();
            self.expect(Token::FatArrow)?;
            self.skip_newlines();

            let body = Box::new(self.parse_expression()?);
            arms.push(IfArm { condition, body });
            self.skip_newlines();

            while self.is(Token::Comma) {
                self.advance();
                self.skip_newlines();
            }

            if has_else {
                break;
            }
        }

        self.expect(Token::RBrace)?;

        if arms.is_empty() {
            return Err("switch block must contain at least one arm".to_string());
        }

        if !has_else {
            arms.push(IfArm {
                condition: IfCondition::Else,
                body: Box::new(Expression::Symbol("none".to_string())),
            });
        }

        let if_expr = Expression::If { arms };
        if let Some(let_expr) = let_binding {
            Ok(Expression::Block(vec![let_expr, if_expr]))
        } else {
            Ok(if_expr)
        }
    }

    fn parse_for_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::For)?;
        self.skip_newlines();

        let iterable = self.parse_expression()?;
        self.skip_newlines();

        let binder = if self.is(Token::Using) {
            self.advance();
            self.skip_newlines();
            Some(self.expect_identifier()?)
        } else {
            None
        };
        self.skip_newlines();

        let body = self.parse_block()?;
        Ok(Expression::For {
            iterable: Box::new(iterable),
            binder,
            body: Box::new(body),
        })
    }

    pub fn parse_type(&mut self) -> Result<Type, String> {
        let first = self.parse_single_type()?;

        let mut idx = self.cursor;
        while idx < self.tokens.len() && self.tokens[idx] == Token::Newline {
            idx += 1;
        }

        if idx < self.tokens.len() && self.tokens[idx] == Token::Pipe {
            self.skip_newlines();
            let mut variants = vec![first];
            while self.is(Token::Pipe) {
                self.advance();
                self.skip_newlines();
                variants.push(self.parse_single_type()?);
                let mut next_idx = self.cursor;
                while next_idx < self.tokens.len() && self.tokens[next_idx] == Token::Newline {
                    next_idx += 1;
                }
                if next_idx < self.tokens.len() && self.tokens[next_idx] == Token::Pipe {
                    self.skip_newlines();
                }
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
                if self.is(Token::Less) {
                    self.advance();
                    self.skip_newlines();
                    let mut type_args = Vec::new();
                    while !self.is(Token::Greater) && !self.is(Token::Eof) {
                        type_args.push(self.parse_type()?);
                        self.skip_newlines();
                        while self.is(Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(Token::Greater)?;
                    Ok(Type::GenericInstance {
                        name,
                        type_arguments: type_args,
                    })
                } else {
                    Ok(Type::Alias(name))
                }
            }
            Token::Symbol(name) => {
                self.advance();
                Ok(Type::Symbol(name))
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
            Token::TypeOpaque => {
                self.advance();
                Ok(Type::Opaque)
            }
            Token::Star => {
                self.advance();
                self.skip_newlines();
                let is_mut = if self.is(Token::Mut) {
                    self.advance();
                    self.skip_newlines();
                    true
                } else {
                    false
                };
                let inner = self.parse_single_type()?;
                Ok(Type::Pointer {
                    element: Box::new(inner),
                    is_mut,
                })
            }
            Token::LBracket => {
                self.advance();
                self.skip_newlines();
                let size = match self.peek().clone() {
                    Token::Integer(value) if value >= 0 => {
                        self.advance();
                        value as usize
                    }
                    token => return Err(format!("expected array size, found {}", token)),
                };
                self.expect(Token::RBracket)?;
                self.skip_newlines();
                let inner = self.parse_single_type()?;
                Ok(Type::Array {
                    element: Box::new(inner),
                    size,
                })
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();

                let mut types = Vec::new();
                if self.is(Token::RParen) {
                    self.advance();
                    return Ok(Type::Tuple(types));
                }
                let first = self.parse_type()?;
                self.skip_newlines();
                if self.is(Token::Comma) {
                    types.push(first);
                    while self.is(Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if self.is(Token::RParen) {
                            break;
                        }
                        types.push(self.parse_type()?);
                        self.skip_newlines();
                    }
                    self.expect(Token::RParen)?;
                    Ok(Type::Tuple(types))
                } else {
                    self.expect(Token::RParen)?;
                    Ok(first)
                }
            }
            Token::LBrace => {
                self.advance();
                self.skip_newlines();

                let mut fields = Vec::new();
                while !self.is(Token::RBrace) && !self.is(Token::Eof) {
                    let field_name = self.expect_field_name()?;
                    if self.is(Token::Colon) || self.is(Token::ColonColon) {
                        self.advance();
                    } else {
                        return Err(format!(
                            "expected ':' after field name, found {}",
                            self.peek()
                        ));
                    }
                    self.skip_newlines();
                    let ty = self.parse_type()?;
                    fields.push(StructFieldType {
                        name: field_name,
                        ty,
                    });
                    self.skip_newlines();
                    while self.is(Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(Token::RBrace)?;
                Ok(Type::Struct(fields))
            }
            Token::FnKeyword => {
                self.advance();
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
                    let mut lookahead = self.cursor + 1;
                    while lookahead < self.tokens.len() && self.tokens[lookahead] == Token::Newline {
                        lookahead += 1;
                    }
                    let is_named = self.tokens.get(lookahead) == Some(&Token::Colon)
                        || self.tokens.get(lookahead) == Some(&Token::ColonColon);
                    let ty = if is_named {
                        let _ = self.expect_field_name()?;
                        self.skip_newlines();
                        self.advance(); // consume ':'
                        self.skip_newlines();
                        self.parse_type()?
                    } else {
                        self.parse_type()?
                    };
                    parameters.push(ty);
                    self.skip_newlines();
                    while self.is(Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(Token::RParen)?;
                self.skip_newlines();
                let return_type = if self.is(Token::Arrow) {
                    self.advance();
                    self.skip_newlines();
                    self.parse_type()?
                } else {
                    Type::Symbol("none".to_string())
                };
                Ok(Type::Function {
                    parameters,
                    return_type: Box::new(return_type),
                    is_variadic,
                })
            }
            token => Err(format!("expected type, found {}", token)),
        }
    }
}
