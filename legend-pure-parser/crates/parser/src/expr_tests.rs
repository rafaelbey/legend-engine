#[cfg(test)]
mod tests {
    use legend_pure_parser_ast::*;
    use legend_pure_parser_lexer::lex_source;
    use crate::{Parser, PluginRegistry};
    use smol_str::SmolStr;
    use std::sync::Arc;

    fn parse_expr(source: &str) -> Expression {
        let registry = Arc::new(PluginRegistry::new());
        let mut parser = Parser::new("test.pure", source, registry);
        // We add a wrapper to just parse an expression instead of a document
        parser.parse_expression(0).expect("Failed to parse expression")
    }

    #[test]
    fn test_literals() {
        assert!(matches!(parse_expr("'hello'"), Expression::Literal(Literal::String(s, _)) if s == "hello"));
        assert!(matches!(parse_expr("42"), Expression::Literal(Literal::Integer(42, _))));
        assert!(matches!(parse_expr("-12.5"), Expression::Literal(Literal::Float(f, _)) if (f - -12.5).abs() < f64::EPSILON));
        assert!(matches!(parse_expr("true"), Expression::Literal(Literal::Boolean(true, _))));
        assert!(matches!(parse_expr("$person"), Expression::Variable(v) if v.name == "person"));
    }

    #[test]
    fn test_function_application() {
        let expr = parse_expr("print('hello')");
        match expr {
            Expression::Application(app) => {
                assert_eq!(app.function_name, "print");
                assert_eq!(app.parameters.len(), 1);
                assert!(matches!(&app.parameters[0], Expression::Literal(Literal::String(s, _)) if s == "hello"));
            }
            _ => panic!("Expected Application"),
        }
    }

    #[test]
    fn test_property_access() {
        let expr = parse_expr("$person.name");
        match expr {
            Expression::Property(p) => {
                assert_eq!(p.property_name, "name");
                assert!(matches!(p.target.as_deref(), Some(Expression::Variable(v)) if v.name == "person"));
            }
            _ => panic!("Expected PropertyAccess"),
        }
    }

    #[test]
    fn test_chained_property_access() {
        let expr = parse_expr("$company.employees.firstName");
        match expr {
            Expression::Property(p) => {
                assert_eq!(p.property_name, "firstName");
                match p.target.as_deref() {
                    Some(Expression::Property(inner)) => {
                        assert_eq!(inner.property_name, "employees");
                        assert!(matches!(inner.target.as_deref(), Some(Expression::Variable(v)) if v.name == "company"));
                    }
                    _ => panic!("Expected nested PropertyAccess"),
                }
            }
            _ => panic!("Expected PropertyAccess"),
        }
    }

    #[test]
    fn test_arithmetic_precedence() {
        // 1 + 2 * 3  -->  1 + (2 * 3)
        let expr = parse_expr("1 + 2 * 3");
        match expr {
            Expression::ArithmeticOp { op, left, right } => {
                assert_eq!(op, ArithOp::Add);
                assert!(matches!(*left, Expression::Literal(Literal::Integer(1, _))));
                match *right {
                    Expression::ArithmeticOp { op: inner_op, left: inner_left, right: inner_right } => {
                        assert_eq!(inner_op, ArithOp::Multiply);
                        assert!(matches!(*inner_left, Expression::Literal(Literal::Integer(2, _))));
                        assert!(matches!(*inner_right, Expression::Literal(Literal::Integer(3, _))));
                    }
                    _ => panic!("Expected Multiply as right child"),
                }
            }
            _ => panic!("Expected ArithmeticOp Add"),
        }
    }

    #[test]
    fn test_parentheses_precedence() {
        // (1 + 2) * 3  -->  (1 + 2) * 3
        let expr = parse_expr("(1 + 2) * 3");
        match expr {
            Expression::ArithmeticOp { op, left, right } => {
                assert_eq!(op, ArithOp::Multiply);
                assert!(matches!(*right, Expression::Literal(Literal::Integer(3, _))));
                match *left {
                    Expression::ArithmeticOp { op: inner_op, left: inner_left, right: inner_right } => {
                        assert_eq!(inner_op, ArithOp::Add);
                        assert!(matches!(*inner_left, Expression::Literal(Literal::Integer(1, _))));
                        assert!(matches!(*inner_right, Expression::Literal(Literal::Integer(2, _))));
                    }
                    _ => panic!("Expected Add as left child"),
                }
            }
            _ => panic!("Expected ArithmeticOp Multiply"),
        }
    }

    #[test]
    fn test_method_chaining_arrow() {
        // $person->manager()  -->  manager($person)
        let expr = parse_expr("$person->manager()");
        match expr {
            Expression::Application(app) => {
                assert_eq!(app.function_name, "manager");
                assert_eq!(app.parameters.len(), 1);
                assert!(matches!(&app.parameters[0], Expression::Variable(v) if v.name == "person"));
            }
            _ => panic!("Expected Application"),
        }
    }

    #[test]
    fn test_collections() {
        let expr = parse_expr("['a', 'b']");
        match expr {
            Expression::Collection(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Expression::Literal(Literal::String(s, _)) if s == "a"));
                assert!(matches!(&items[1], Expression::Literal(Literal::String(s, _)) if s == "b"));
            }
            _ => panic!("Expected Collection"),
        }
    }

    #[test]
    fn test_lambdas() {
        // $x | $x + 1
        let expr = parse_expr("$x | $x + 1");
        match expr {
            Expression::Lambda(l) => {
                assert_eq!(l.parameters.len(), 1);
                assert_eq!(l.parameters[0].name, "x");
                assert_eq!(l.body.len(), 1);
                assert!(matches!(l.body[0], Expression::ArithmeticOp { op: ArithOp::Add, .. }));
            }
            _ => panic!("Expected Lambda"),
        }
    }

    // --- Negative Test Cases ---

    fn parse_expr_error(source: &str) -> crate::ParseError {
        let registry = Arc::new(PluginRegistry::new());
        let mut parser = Parser::new("test_err.pure", source, registry);
        match parser.parse_expression(0) {
            Err(e) => e,
            Ok(_) => panic!("Expected parsing to fail"),
        }
    }

    #[test]
    fn test_err_missing_closing_paren() {
        assert!(matches!(parse_expr_error("(1 + 2"), crate::ParseError::UnexpectedEof));
    }

    #[test]
    fn test_err_missing_right_operand() {
        assert!(matches!(parse_expr_error("1 + "), crate::ParseError::UnexpectedEof));
    }

    #[test]
    fn test_err_missing_property_name() {
        assert!(matches!(parse_expr_error("$person."), crate::ParseError::UnexpectedEof | crate::ParseError::UnexpectedToken(_, _)));
    }

    #[test]
    fn test_err_missing_method_name() {
        let err = parse_expr_error("$person->()");
        assert!(matches!(err, crate::ParseError::UnexpectedToken(crate::Token::LParen, _)));
    }

    #[test]
    fn test_err_missing_closing_bracket() {
        assert!(matches!(parse_expr_error("[1, 2"), crate::ParseError::UnexpectedEof));
    }
}
