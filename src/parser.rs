use crate::ast::{
    BinaryOperator, ComparisonOperator, ConditionalBranch, Definition, Expression,
    ExternalFunction, Parameter, Program, Signature, Statement, Type, UnaryOperator,
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
        let mut extern_functions = Vec::new();
        let mut definitions = Vec::new();

        self.skip_newlines();

        while !self.is(Token::Eof) {
            match self.peek() {
                Token::At => {
                    self.advance();
                    let directive = self.expect_identifier()?;

                    match directive.as_str() {
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
                                    "expected signature expression for @extern '{}', found {:?}",
                                    name, expr
                                ));
                            }
                        }
                        "fn" => {
                            self.skip_newlines();
                            let name = self.expect_identifier()?;
                            self.skip_newlines();
                            let value = self.parse_expression()?;
                            self.consume_optional_newline();
                            definitions.push(Definition { name, value });
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
                                "unknown top-level decorator '@{}', expected @fn, @extern, or @main",
                                other
                            ));
                        }
                    }
                }

                Token::Newline => {
                    self.skip_newlines();
                }

                _ => {
                    return Err(format!("unexpected token at top level: {:?}", self.peek()));
                }
            }

            self.skip_newlines();
        }

        if !definitions.iter().any(|d| d.name == "main") {
            return Err("missing required '@main' definition".to_string());
        }

        Ok(Program {
            variables: Vec::new(),
            extern_functions,
            definitions,
        })
    }

    pub fn parse_signature(&mut self) -> Result<Signature, String> {
        self.expect(Token::LParen)?;

        let mut parameters = Vec::new();

        if !self.is(Token::RParen) {
            loop {
                let name = self.expect_identifier()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;

                parameters.push(Parameter { name, ty });

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

        let return_type = if self.is(Token::To) {
            self.advance();
            self.parse_type()?
        } else {
            Type::Void
        };

        Ok(Signature {
            parameters,
            return_type: Box::new(return_type),
            is_variadic,
        })
    }

    fn parse_block_expression(&mut self) -> Result<Expression, String> {
        self.expect(Token::Do)?;

        let signature = if self.is(Token::LParen) {
            Some(self.parse_signature()?)
        } else if self.is(Token::To) {
            self.advance();
            let return_type = self.parse_type()?;
            Some(Signature {
                parameters: Vec::new(),
                return_type: Box::new(return_type),
                is_variadic: false,
            })
        } else {
            None
        };

        self.skip_newlines();
        let body = self.parse_statements_until_done()?;

        Ok(Expression::Block { signature, body })
    }

    fn parse_statements_until_done(&mut self) -> Result<Vec<Statement>, String> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.is(Token::Eof)
            && !self.is(Token::Done)
            && !self.is(Token::Elif)
            && !self.is(Token::Else)
        {
            statements.push(self.parse_statement()?);
            self.skip_newlines();
        }

        if !self.is(Token::Elif) && !self.is(Token::Else) {
            self.expect(Token::Done)?;
        }

        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.peek() {
            Token::Const => {
                self.advance();
                let name = self.expect_identifier()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                self.consume_optional_newline();
                Ok(Statement::Declare {
                    is_mutable: false,
                    name,
                    ty,
                })
            }

            Token::Var => {
                self.advance();
                let name = self.expect_identifier()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                self.consume_optional_newline();
                Ok(Statement::Declare {
                    is_mutable: true,
                    name,
                    ty,
                })
            }

            Token::If => self.parse_if_statement(),

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

            _ => {
                let expression = self.parse_expression()?;

                if self.is(Token::Arrow) {
                    let mut targets = Vec::new();
                    while self.is(Token::Arrow) {
                        self.advance();
                        targets.push(self.expect_identifier()?);
                    }
                    self.consume_optional_newline();
                    Ok(Statement::Assign {
                        expression,
                        targets,
                    })
                } else {
                    self.consume_optional_newline();
                    Ok(Statement::Discard(expression))
                }
            }
        }
    }

    fn parse_if_statement(&mut self) -> Result<Statement, String> {
        self.expect(Token::If)?;
        let condition = self.parse_expression()?;
        self.skip_newlines();
        self.expect(Token::Do)?;
        self.skip_newlines();
        let body = self.parse_statements_until_done()?;

        let mut branches = vec![ConditionalBranch { condition, body }];
        self.skip_newlines();

        while self.is(Token::Elif) {
            self.advance();
            let elif_condition = self.parse_expression()?;
            self.skip_newlines();
            self.expect(Token::Do)?;
            self.skip_newlines();
            let elif_body = self.parse_statements_until_done()?;

            branches.push(ConditionalBranch {
                condition: elif_condition,
                body: elif_body,
            });
            self.skip_newlines();
        }

        let else_body = if self.is(Token::Else) {
            self.advance();
            self.skip_newlines();
            self.expect(Token::Do)?;
            self.skip_newlines();
            Some(self.parse_statements_until_done()?)
        } else {
            None
        };

        Ok(Statement::If {
            branches,
            else_body,
        })
    }

    fn parse_expression(&mut self) -> Result<Expression, String> {
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
                    expression = Expression::Call {
                        target: Box::new(expression),
                        arguments,
                    };
                }

                Token::Dot => {
                    self.advance();
                    let name = self.expect_identifier()?;

                    if self.is(Token::LParen) {
                        self.advance();
                        let mut arguments = vec![expression];

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
                        expression = Expression::Call {
                            target: Box::new(Expression::Identifier(name)),
                            arguments,
                        };
                    } else {
                        expression = Expression::Member {
                            expression: Box::new(expression),
                            field: name,
                        };
                    }
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

    fn parse_primary(&mut self) -> Result<Expression, String> {
        match self.peek().clone() {
            Token::Do => self.parse_block_expression(),

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

            Token::Identifier(name) => {
                self.advance();
                Ok(Expression::Identifier(name))
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
                if self.is_signature_start() {
                    let sig = self.parse_signature()?;
                    Ok(Expression::Signature(sig))
                } else {
                    self.advance();
                    let expression = self.parse_expression()?;
                    self.expect(Token::RParen)?;
                    Ok(expression)
                }
            }

            token => Err(format!("unexpected token in expression: {:?}", token)),
        }
    }

    fn is_signature_start(&self) -> bool {
        if self.position + 1 >= self.tokens.len() {
            return false;
        }

        if let Token::RParen = &self.tokens[self.position + 1] {
            return true;
        }

        if self.position + 2 < self.tokens.len() {
            matches!(
                (
                    &self.tokens[self.position + 1],
                    &self.tokens[self.position + 2]
                ),
                (Token::Identifier(_), Token::Colon)
            )
        } else {
            false
        }
    }

    fn parse_type(&mut self) -> Result<Type, String> {
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
                let element = self.parse_type()?;
                Ok(Type::Pointer {
                    element: Box::new(element),
                })
            }
            Token::LBracket => {
                self.advance();
                if self.is(Token::RBracket) {
                    self.advance();
                    let element = self.parse_type()?;
                    return Ok(Type::Slice {
                        element: Box::new(element),
                    });
                }

                let size = match self.peek().clone() {
                    Token::Integer(value) if value >= 0 => {
                        self.advance();
                        value as usize
                    }
                    token => return Err(format!("expected array size, found {:?}", token)),
                };

                self.expect(Token::RBracket)?;
                let element = self.parse_type()?;
                Ok(Type::Array {
                    element: Box::new(element),
                    size,
                })
            }
            Token::LParen => {
                let sig = self.parse_signature()?;
                let param_types = sig.parameters.into_iter().map(|p| p.ty).collect();
                let return_type = *sig.return_type;

                Ok(Type::Function {
                    parameters: param_types,
                    return_type: Box::new(return_type),
                    is_variadic: sig.is_variadic,
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
