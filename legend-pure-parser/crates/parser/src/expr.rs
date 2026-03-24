use legend_pure_parser_ast::*;
use legend_pure_parser_lexer::Token;
use crate::{ParseError, Parser};
use smol_str::SmolStr;

/// Binding power (precedence) for Pratt parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    Lowest = 0,
    Lambda = 1,      // |
    LogicalOr = 2,   // ||
    LogicalAnd = 3,  // &&
    Equality = 4,    // ==, !=
    Relational = 5,  // <, <=, >, >=
    Additive = 6,    // +, -
    Multiplicative = 7, // *, /
    Prefix = 8,      // -x, !x
    MethodCall = 9,  // ->
    Call = 10,       // ()
    Property = 11,   // .x
}

impl<'a> Parser<'a> {
    pub fn parse_expression(&mut self, precedence: u8) -> Result<Expression, ParseError> {
        let mut left = self.parse_prefix()?;

        while self.pos < self.tokens.len() {
            let next_prec = self.infix_binding_power(&self.tokens[self.pos].token);
            if precedence >= next_prec as u8 {
                break;
            }

            left = self.parse_infix(left)?;
        }

        Ok(left)
    }

    fn infix_binding_power(&self, token: &Token) -> Precedence {
        match token {
            Token::Dot => Precedence::Property,
            Token::LParen => Precedence::Call,
            Token::Arrow => Precedence::MethodCall,
            Token::Star | Token::Slash => Precedence::Multiplicative,
            Token::Plus | Token::Minus => Precedence::Additive,
            Token::Lt | Token::Lte | Token::Gt | Token::Gte => Precedence::Relational,
            Token::EqEq | Token::NotEq => Precedence::Equality,
            Token::And => Precedence::LogicalAnd,
            Token::Or => Precedence::LogicalOr,
            Token::Pipe => Precedence::Lambda,
            _ => Precedence::Lowest,
        }
    }

    fn parse_prefix(&mut self) -> Result<Expression, ParseError> {
        if self.pos >= self.tokens.len() {
            return Err(ParseError::UnexpectedEof);
        }

        let token = self.tokens[self.pos].token.clone();
        
        // Handle Variable or Variable declarations (for lambdas)
        if let Token::Variable(v) = &token {
            // This could be $x, or possibly start of lambda `$x | ...`
            self.pos += 1;
            
            // Check if this is the start of a lambda `$x |`
            if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Pipe {
                return self.parse_lambda_prefix(vec![Variable {
                    name: v.clone(),
                    variable_type: None,
                    multiplicity: None,
                    source_info: SourceInfo::dummy(),
                }]);
            }
            
            return Ok(Expression::Variable(Variable {
                name: v.clone(),
                variable_type: None,
                multiplicity: None,
                source_info: SourceInfo::dummy(),
            }));
        }

        self.pos += 1;

        match token {
            Token::StringLit(s) => Ok(Expression::Literal(Literal::String(s, SourceInfo::dummy()))),
            Token::Integer(i) => Ok(Expression::Literal(Literal::Integer(i, SourceInfo::dummy()))),
            Token::Float(f) => Ok(Expression::Literal(Literal::Float(f, SourceInfo::dummy()))),
            Token::Bool(b) => Ok(Expression::Literal(Literal::Boolean(b, SourceInfo::dummy()))),
            Token::Ident(name) => {
                // If followed by LParen, it's a function call prefix
                if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::LParen {
                    self.pos += 1; // Consume '('
                    let mut params = Vec::new();
                    if self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RParen {
                        params.push(self.parse_expression(0)?);
                        while self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Comma {
                            self.pos += 1; // Consume ','
                            params.push(self.parse_expression(0)?);
                        }
                    }
                    self.expect_token(Token::RParen)?;
                    Ok(Expression::Application(FunctionApplication {
                        function_name: name,
                        parameters: params,
                        source_info: SourceInfo::dummy(),
                    }))
                } else {
                    // Just an identifier (might not be valid in strict Pure outside of types, but handling for now)
                    Err(ParseError::UnexpectedToken(Token::Ident(name), self.pos - 1))
                }
            }
            Token::LBracket => {
                // Collection [1, 2, 3]
                let mut items = Vec::new();
                if self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RBracket {
                    items.push(self.parse_expression(0)?);
                    while self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Comma {
                        self.pos += 1;
                        items.push(self.parse_expression(0)?);
                    }
                }
                self.expect_token(Token::RBracket)?;
                Ok(Expression::Collection(items))
            }
            Token::LParen => {
                // Grouping ( ... )
                let expr = self.parse_expression(0)?;
                self.expect_token(Token::RParen)?;
                Ok(expr)
            }
            Token::Pipe => {
                // Lambda without params: `| 'hello'`
                let body = vec![self.parse_expression(0)?]; // simplified, pure lambdas can have multiple statements
                Ok(Expression::Lambda(Lambda {
                    parameters: vec![],
                    body,
                    source_info: SourceInfo::dummy(),
                }))
            }
            Token::Bang => {
                let right = self.parse_expression(Precedence::Prefix as u8)?;
                Ok(Expression::Not(Box::new(right)))
            }
            _ => Err(ParseError::UnexpectedToken(token, self.pos - 1)),
        }
    }

    fn parse_lambda_prefix(&mut self, params: Vec<Variable>) -> Result<Expression, ParseError> {
        self.expect_token(Token::Pipe)?;
        let mut body = Vec::new();
        body.push(self.parse_expression(0)?); // Simplified, usually a list separated by `;`
        
        Ok(Expression::Lambda(Lambda {
            parameters: params,
            body,
            source_info: SourceInfo::dummy(),
        }))
    }

    fn parse_infix(&mut self, left: Expression) -> Result<Expression, ParseError> {
        let token = self.tokens[self.pos].token.clone();
        let precedence = self.infix_binding_power(&token);
        self.pos += 1;

        match token {
            Token::Plus | Token::Minus | Token::Star | Token::Slash => {
                let right = self.parse_expression(precedence as u8)?;
                let op = match token {
                    Token::Plus => ArithOp::Add,
                    Token::Minus => ArithOp::Subtract,
                    Token::Star => ArithOp::Multiply,
                    Token::Slash => ArithOp::Divide,
                    _ => unreachable!(),
                };
                Ok(Expression::ArithmeticOp { op, left: Box::new(left), right: Box::new(right) })
            }
            Token::EqEq | Token::NotEq | Token::Lt | Token::Lte | Token::Gt | Token::Gte => {
                let right = self.parse_expression(precedence as u8)?;
                let op = match token {
                    Token::EqEq => CompOp::Eq,
                    Token::NotEq => CompOp::NotEq,
                    Token::Lt => CompOp::Lt,
                    Token::Lte => CompOp::Lte,
                    Token::Gt => CompOp::Gt,
                    Token::Gte => CompOp::Gte,
                    _ => unreachable!(),
                };
                Ok(Expression::ComparisonOp { op, left: Box::new(left), right: Box::new(right) })
            }
            Token::And | Token::Or => {
                let right = self.parse_expression(precedence as u8)?;
                let op = match token {
                    Token::And => BoolOp::And,
                    Token::Or => BoolOp::Or,
                    _ => unreachable!(),
                };
                Ok(Expression::BooleanOp { op, left: Box::new(left), right: Box::new(right) })
            }
            Token::Dot => {
                // Property access e.g., `$person.name`
                if self.pos >= self.tokens.len() {
                    return Err(ParseError::UnexpectedEof);
                }
                let prop_name = match &self.tokens[self.pos].token {
                    Token::Ident(id) => id.clone(),
                    _ => return Err(ParseError::UnexpectedToken(self.tokens[self.pos].token.clone(), self.pos))
                };
                self.pos += 1;
                Ok(Expression::Property(PropertyAccess {
                    property_name: prop_name,
                    target: Some(Box::new(left)),
                    source_info: SourceInfo::dummy(),
                }))
            }
            Token::Arrow => {
                // Method chaining e.g., `$person->manager()`
                if self.pos >= self.tokens.len() {
                    return Err(ParseError::UnexpectedEof);
                }
                let func_name = match &self.tokens[self.pos].token {
                    Token::Ident(id) => id.clone(),
                    _ => return Err(ParseError::UnexpectedToken(self.tokens[self.pos].token.clone(), self.pos))
                };
                self.pos += 1;
                
                self.expect_token(Token::LParen)?;
                let mut params = vec![left]; // LH side is the first parameter
                
                if self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RParen {
                    params.push(self.parse_expression(0)?);
                    while self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Comma {
                        self.pos += 1;
                        params.push(self.parse_expression(0)?);
                    }
                }
                self.expect_token(Token::RParen)?;
                
                Ok(Expression::Application(FunctionApplication {
                    function_name: func_name,
                    parameters: params,
                    source_info: SourceInfo::dummy(),
                }))
            }
            _ => Err(ParseError::UnexpectedToken(token, self.pos - 1)),
        }
    }
}
