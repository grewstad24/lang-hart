#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    Bool,

    Array { element: Box<Type>, size: usize },

    Pointer { element: Box<Type> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpression {
    Concrete(Type),
    Of(Box<Expression>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionKind {
    Pure,
    Impure,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub ty: TypeExpression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeExpression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSignature {
    pub parameters: Vec<TypeExpression>,
    pub return_type: TypeExpression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalFunction {
    pub name: String,
    pub signature: FunctionSignature,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub kind: FunctionKind,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: TypeExpression,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Main {
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub variables: Vec<Variable>,
    pub extern_functions: Vec<ExternalFunction>,
    pub main: Main,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Bind {
        expression: Expression,
        names: Vec<String>,
    },

    Return(Expression),

    Exit(Expression),

    Unsafe(Vec<Statement>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Integer(i64),

    Boolean(bool),

    String(Vec<u8>),

    Identifier(String),

    Array(Vec<Expression>),

    TypeOf(Box<Expression>),

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
        ty: TypeExpression,
    },

    Call {
        name: String,
        arguments: Vec<Expression>,
    },

    Index {
        expression: Box<Expression>,
        index: Box<Expression>,
    },

    AddressOf(Box<Expression>),
}

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
