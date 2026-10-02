use crate::ast::{
    BinaryOperator, ComparisonOperator, Expression, ExternalFunction, Function, FunctionKind,
    FunctionSignature, Main, Parameter, Program, Statement, Type, TypeExpression, UnaryOperator,
    Variable,
};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut variables = Vec::new();
        let mut extern_functions = Vec::new();
        let mut main = None;
        let mut functions = Vec::new();
        let mut has_data = false;

        self.skip_newlines();

        while !self.is(Token::Eof) {
            match self.peek() {
                Token::Data => {
                    if has_data {
                        return Err("duplicate data block".to_string());
                    }
                    has_data = true;
                    variables = self.parse_data_block()?;
                }

                Token::Extern => {
                    extern_functions.push(self.parse_extern_function()?);
                }

                Token::Main => {
                    if main.is_some() {
                        return Err("duplicate main block".to_string());
                    }

                    main = Some(self.parse_main()?);
                }

                Token::Pure | Token::Impure => {
                    functions.push(self.parse_function()?);
                }

                Token::Newline => {
                    self.skip_newlines();
                }

                Token::Dedent | Token::Indent => {
                    return Err(format!(
                        "unexpected indentation at top level: {:?}",
                        self.peek()
                    ));
                }

                _ => {
                    return Err(format!("unexpected token at top level: {:?}", self.peek()));
                }
            }

            self.skip_newlines();
        }

        let main = main.ok_or_else(|| "missing main block".to_string())?;

        Ok(Program {
            variables,
            extern_functions,
            main,
            functions,
        })
    }

    fn parse_data_block(&mut self) -> Result<Vec<Variable>, String> {
        self.expect(Token::Data)?;
        self.expect(Token::Colon)?;
        self.consume_optional_newline();
        self.expect(Token::Indent)?;

        let mut variables = Vec::new();
        self.skip_newlines();

        while !self.is(Token::Eof)
            && !self.is(Token::Dedent)
            && !self.is(Token::Extern)
            && !self.is(Token::Main)
            && !self.is(Token::Pure)
            && !self.is(Token::Impure)
            && !self.is(Token::Data)
        {
            let ty = self.parse_type_expression()?;
            let name = self.expect_identifier()?;
            self.consume_optional_newline();

            variables.push(Variable { name, ty });
            self.skip_newlines();
        }

        self.expect(Token::Dedent)?;

        Ok(variables)
    }

    fn parse_extern_function(&mut self) -> Result<ExternalFunction, String> {
        self.expect(Token::Extern)?;

        let name = self.expect_identifier()?;

        self.expect(Token::Colon)?;
        self.expect(Token::LParen)?;

        let mut parameters = Vec::new();

        if !self.is(Token::RParen) {
            loop {
                parameters.push(self.parse_type_expression()?);

                if self.is(Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        self.expect(Token::RParen)?;

        let is_variadic = if self.is(Token::PlusPlus) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(Token::To)?;

        let return_type = self.parse_type_expression()?;

        self.consume_optional_newline();

        Ok(ExternalFunction {
            name,
            signature: FunctionSignature {
                parameters,
                return_type,
                is_variadic,
            },
        })
    }

    fn parse_main(&mut self) -> Result<Main, String> {
        self.expect(Token::Main)?;
        self.expect(Token::Colon)?;

        self.consume_optional_newline();

        let body = self.parse_block(true)?;

        Ok(Main { body })
    }

    fn parse_function(&mut self) -> Result<Function, String> {
        let kind = match self.peek() {
            Token::Pure => {
                self.advance();
                FunctionKind::Pure
            }

            Token::Impure => {
                self.advance();
                FunctionKind::Impure
            }

            token => {
                return Err(format!("expected function kind, found {:?}", token));
            }
        };

        let name = self.expect_identifier()?;

        self.expect(Token::LParen)?;

        let mut parameters = Vec::new();

        if !self.is(Token::RParen) {
            loop {
                let ty = self.parse_type_expression()?;
                let name = self.expect_identifier()?;

                parameters.push(Parameter { name, ty });

                if self.is(Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        self.expect(Token::RParen)?;
        self.expect(Token::To)?;

        let return_type = self.parse_type_expression()?;

        self.expect(Token::Colon)?;
        self.consume_optional_newline();

        let body = self.parse_block(true)?;

        Ok(Function {
            kind,
            name,
            parameters,
            return_type,
            body,
        })
    }

    fn parse_block(&mut self, require_indent: bool) -> Result<Vec<Statement>, String> {
        let has_indent = self.is(Token::Indent);

        if require_indent && !has_indent {
            return Err("expected an indented block".to_string());
        }

        if has_indent {
            self.advance();
        }

        let mut statements = Vec::new();

        self.skip_newlines();

        while !self.is(Token::Eof)
            && !self.is(Token::Dedent)
            && !self.is(Token::Data)
            && !self.is(Token::Extern)
            && !self.is(Token::Main)
            && !self.is(Token::Pure)
            && !self.is(Token::Impure)
        {
            statements.push(self.parse_statement()?);
            self.skip_newlines();
        }

        if has_indent {
            self.expect(Token::Dedent)?;
        }

        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.peek() {
            Token::Return => {
                self.advance();

                let expression = self.parse_expression()?;

                self.consume_optional_newline();

                Ok(Statement::Return(expression))
            }

            Token::Exit => {
                self.advance();

                let expression = self.parse_expression()?;

                self.consume_optional_newline();

                Ok(Statement::Exit(expression))
            }

            Token::Underscore => {
                self.advance();

                let expression = self.parse_expression()?;

                self.consume_optional_newline();

                Ok(Statement::Discard(expression))
            }

            Token::Unsafe => self.parse_unsafe_block(),

            _ => {
                let expression = self.parse_expression()?;

                self.expect(Token::Arrow)?;

                let mut names = Vec::new();

                names.push(self.expect_identifier()?);

                while self.is(Token::Arrow) {
                    self.advance();
                    names.push(self.expect_identifier()?);
                }

                self.consume_optional_newline();

                Ok(Statement::Bind { expression, names })
            }
        }
    }

    fn parse_unsafe_block(&mut self) -> Result<Statement, String> {
        self.expect(Token::Unsafe)?;
        self.expect(Token::Colon)?;

        self.consume_optional_newline();

        let body = self.parse_block(true)?;

        Ok(Statement::Unsafe(body))
    }

    fn parse_expression(&mut self) -> Result<Expression, String> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression, String> {
        let mut expression = self.parse_additive()?;

        loop {
            let operator = match self.peek() {
                Token::EqualEqual => ComparisonOperator::Equal,
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

                Token::LBracket => {
                    self.advance();

                    let index = self.parse_expression()?;

                    self.expect(Token::RBracket)?;

                    expression = Expression::Index {
                        expression: Box::new(expression),
                        index: Box::new(index),
                    };
                }

                Token::As => {
                    self.advance();

                    let ty = self.parse_type_expression()?;

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

            Token::Hash => {
                self.advance();

                let expression = self.parse_expression()?;

                Ok(Expression::TypeOf(Box::new(expression)))
            }

            Token::Identifier(name) => {
                self.advance();

                if self.is(Token::LParen) {
                    self.advance();

                    let mut arguments = Vec::new();

                    if !self.is(Token::RParen) {
                        loop {
                            arguments.push(self.parse_expression()?);

                            if self.is(Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }

                    self.expect(Token::RParen)?;

                    Ok(Expression::Call { name, arguments })
                } else {
                    Ok(Expression::Identifier(name))
                }
            }

            Token::LBracket => {
                self.advance();

                let mut elements = Vec::new();

                if !self.is(Token::RBracket) {
                    loop {
                        elements.push(self.parse_expression()?);

                        if self.is(Token::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }

                self.expect(Token::RBracket)?;

                Ok(Expression::Array(elements))
            }

            Token::LParen => {
                self.advance();

                let expression = self.parse_expression()?;

                self.expect(Token::RParen)?;

                Ok(expression)
            }

            token => Err(format!("unexpected token in expression: {:?}", token)),
        }
    }

    fn parse_type_expression(&mut self) -> Result<TypeExpression, String> {
        if self.is(Token::Hash) {
            self.advance();

            let expression = self.parse_expression()?;

            Ok(TypeExpression::Of(Box::new(expression)))
        } else {
            Ok(TypeExpression::Concrete(self.parse_concrete_type()?))
        }
    }

    fn parse_concrete_type(&mut self) -> Result<Type, String> {
        match self.peek() {
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

            Token::TypeU8 => {
                self.advance();
                Ok(Type::U8)
            }

            Token::TypeBool => {
                self.advance();
                Ok(Type::Bool)
            }

            Token::Star => {
                self.advance();

                let element = self.parse_concrete_type()?;

                Ok(Type::Pointer {
                    element: Box::new(element),
                })
            }

            Token::LBracket => {
                self.advance();

                let size = match self.peek().clone() {
                    Token::Integer(value) if value >= 0 => {
                        self.advance();
                        value as usize
                    }

                    token => {
                        return Err(format!("expected array size, found {:?}", token));
                    }
                };

                self.expect(Token::RBracket)?;

                let element = self.parse_concrete_type()?;

                Ok(Type::Array {
                    element: Box::new(element),
                    size,
                })
            }

            token => Err(format!("expected type, found {:?}", token)),
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

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&expected) {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, found {:?}", expected, self.peek()))
        }
    }

    fn is(&self, token: Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(&token)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn advance(&mut self) {
        if self.position < self.tokens.len() {
            self.position += 1;
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
}
