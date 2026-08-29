use odata::query::*;

#[test]
fn tokenizes_simple_filter() {
    let tokens = tokenize("Title eq 'Hello World' and Top gt 10").unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Identifier("Title".into()),
            Token::Eq,
            Token::String("Hello World".into()),
            Token::And,
            Token::Identifier("Top".into()),
            Token::Gt,
            Token::Number(10.0),
        ]
    );
}

#[test]
fn tokenizes_nested_path_and_function() {
    let tokens = tokenize("contains(Category/Name, 'Foo')").unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Identifier("contains".into()),
            Token::LParen,
            Token::Identifier("Category".into()),
            Token::Slash,
            Token::Identifier("Name".into()),
            Token::Comma,
            Token::String("Foo".into()),
            Token::RParen,
        ]
    );
}

#[test]
fn tokenizes_guid() {
    let tokens = tokenize("ID eq 12345678-1234-1234-1234-123456789012").unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Identifier("ID".into()),
            Token::Eq,
            Token::Guid("12345678-1234-1234-1234-123456789012".into()),
        ]
    );
}

#[test]
pub fn parse1() {
    let f = Expr::parse("a eq 1").unwrap();

    match f {
        Expr::BinaryOp {
            op: BinOp::Eq,
            left,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (Expr::Identifier(a), Expr::Number(n)) => {
                assert_eq!(a, "a");
                assert_eq!(n, &1f64);
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn parses_simple_comparison() {
    let expr = parse_filter("Title eq 'Hello'").unwrap();
    assert_eq!(
        expr,
        Expr::BinaryOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Identifier("Title".into())),
            right: Box::new(Expr::String("Hello".into())),
        }
    );
}

#[test]
fn respects_and_or_precedence() {
    // and bindet stärker als or:
    // A eq 1 or B eq 2 and C eq 3  ==  A eq 1 or (B eq 2 and C eq 3)
    let expr = parse_filter("A eq 1 or B eq 2 and C eq 3").unwrap();

    let expected = Expr::BinaryOp {
        op: BinOp::Or,
        left: Box::new(Expr::BinaryOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Identifier("A".into())),
            right: Box::new(Expr::Number(1.0)),
        }),
        right: Box::new(Expr::BinaryOp {
            op: BinOp::And,
            left: Box::new(Expr::BinaryOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Identifier("B".into())),
                right: Box::new(Expr::Number(2.0)),
            }),
            right: Box::new(Expr::BinaryOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Identifier("C".into())),
                right: Box::new(Expr::Number(3.0)),
            }),
        }),
    };

    assert_eq!(expr, expected);
}

#[test]
fn parses_parentheses_override_precedence() {
    let expr = parse_filter("(A eq 1 or B eq 2) and C eq 3").unwrap();
    // äußerster Knoten muss And sein, nicht Or
    match expr {
        Expr::BinaryOp { op: BinOp::And, .. } => {}
        other => panic!("Erwartete And als Wurzel, bekam: {other:?}"),
    }
}

#[test]
fn parses_not() {
    let expr = parse_filter("not (Active eq true)").unwrap();
    match expr {
        Expr::UnaryNot(inner) => match *inner {
            Expr::BinaryOp { op: BinOp::Eq, .. } => {}
            other => panic!("Unerwarteter innerer Ausdruck: {other:?}"),
        },
        other => panic!("Erwartete UnaryNot, bekam: {other:?}"),
    }
}

#[test]
fn parses_function_call() {
    let expr = parse_filter("contains(Title, 'foo')").unwrap();
    assert_eq!(
        expr,
        Expr::FunctionCall {
            name: "contains".into(),
            args: vec![Expr::Identifier("Title".into()), Expr::String("foo".into()),],
        }
    );
}

#[test]
fn parses_path_navigation() {
    let expr = parse_filter("Category/Name eq 'Foo'").unwrap();
    assert_eq!(
        expr,
        Expr::BinaryOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Path(vec!["Category".into(), "Name".into()])),
            right: Box::new(Expr::String("Foo".into())),
        }
    );
}

#[test]
fn parses_nested_function_and_path() {
    let expr = parse_filter("contains(Category/Name, 'Foo') and Top gt 10").unwrap();
    match expr {
        Expr::BinaryOp {
            op: BinOp::And,
            left,
            right,
        } => {
            assert_eq!(
                *left,
                Expr::FunctionCall {
                    name: "contains".into(),
                    args: vec![
                        Expr::Path(vec!["Category".into(), "Name".into()]),
                        Expr::String("Foo".into()),
                    ],
                }
            );
            assert_eq!(
                *right,
                Expr::BinaryOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Identifier("Top".into())),
                    right: Box::new(Expr::Number(10.0)),
                }
            );
        }
        other => panic!("Erwartete And, bekam: {other:?}"),
    }
}

#[test]
fn error_on_unbalanced_parens() {
    let result = parse_filter("(A eq 1");
    assert!(result.is_err());
}

#[test]
fn error_on_trailing_garbage() {
    let result = parse_filter("A eq 1 )");
    assert!(result.is_err());
}
