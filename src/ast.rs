#[derive(Debug, Clone, PartialEq, Eq)]
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
    Function {
        parameters: Vec<Type>,
        return_type: Box<Type>,
        is_variadic: bool,
    },
    Array {
        element: Box<Type>,
        size: usize,
    },
    Pointer {
        element: Box<Type>,
    },
    Slice {
        element: Box<Type>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalFunction {
    pub name: String,
    pub signature: Signature,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub variables: Vec<()>,
    pub extern_functions: Vec<ExternalFunction>,
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalBranch {
    pub condition: Expression,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Declare {
        is_mutable: bool,
        name: String,
        ty: Type,
    },
    Assign {
        expression: Expression,
        targets: Vec<String>,
    },
    Discard(Expression),
    Return(Expression),
    Exit(Expression),
    If {
        branches: Vec<ConditionalBranch>,
        else_body: Option<Vec<Statement>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Integer(i64),
    Boolean(bool),
    String(Vec<u8>),
    Identifier(String),
    Array(Vec<Expression>),
    Signature(Signature),
    Block {
        signature: Option<Signature>,
        body: Vec<Statement>,
    },
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
