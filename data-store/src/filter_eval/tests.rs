use super::*;
use serde_json::json;

fn record() -> DataRecord {
    DataRecord::from_json(&json!({
        "ID": "00000000-0000-0000-0000-000000000001",
        "IsActiveEntity": true,
        "Amount": 150,
        "Status": "Released",
    }))
    .unwrap()
}

fn ident(name: &str) -> Expr {
    Expr::Identifier(name.into())
}

fn bin(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[test]
fn comparison_operators_work() {
    let r = record();
    for (op, expected) in [
        (BinOp::Gt, true),
        (BinOp::Ge, true),
        (BinOp::Lt, false),
        (BinOp::Le, false),
    ] {
        let e = bin(op, ident("Amount"), Expr::Number(100.0));
        assert_eq!(evaluate(&r, &e), json!(expected), "{op:?}");
    }
}

#[test]
fn comparison_on_incompatible_types_is_null() {
    let e = bin(BinOp::Gt, ident("Status"), Expr::Number(100.0));
    assert_eq!(evaluate(&record(), &e), Value::Null);
}

#[test]
fn string_functions() {
    let r = record();
    let call = |name: &str, args: Vec<Expr>| {
        evaluate(
            &r,
            &Expr::FunctionCall {
                name: name.into(),
                args,
            },
        )
    };
    assert_eq!(
        call(
            "contains",
            vec![ident("Status"), Expr::String("lea".into())]
        ),
        json!(true)
    );
    assert_eq!(
        call(
            "startswith",
            vec![ident("Status"), Expr::String("Rel".into())]
        ),
        json!(true)
    );
    assert_eq!(
        call(
            "endswith",
            vec![ident("Status"), Expr::String("sed".into())]
        ),
        json!(true)
    );
    assert_eq!(call("tolower", vec![ident("Status")]), json!("released"));
    assert_eq!(call("length", vec![ident("Status")]), json!(8.0));
    assert_eq!(
        call(
            "substringof",
            vec![Expr::String("lea".into()), ident("Status")]
        ),
        json!(true)
    );
}

#[test]
fn and_or_short_circuit_on_unknown() {
    let r = record();
    let unknown = bin(BinOp::Gt, ident("Status"), Expr::Number(1.0));
    let f = bin(BinOp::And, Expr::Bool(false), unknown.clone());
    assert_eq!(evaluate(&r, &f), json!(false));
    let t = bin(BinOp::Or, Expr::Bool(true), unknown);
    assert_eq!(evaluate(&r, &t), json!(true));
}

#[test]
fn multi_segment_paths_resolve_without_panicking() {
    let mut r = record();
    r["Customer"] = json!({ "Name": "Mueller" });
    assert_eq!(evaluate(&r, &ident("Customer/Name")), json!("Mueller"));
    assert_eq!(evaluate(&r, &ident("Customer/Nope")), Value::Null);
}

#[test]
fn unknown_function_is_null_not_panic() {
    let e = Expr::FunctionCall {
        name: "geo.distance".into(),
        args: vec![],
    };
    assert_eq!(evaluate(&record(), &e), Value::Null);
}

fn video_record(tags: Value) -> DataRecord {
    DataRecord::from_json(&json!({
        "ID": "00000000-0000-0000-0000-000000000002",
        "IsActiveEntity": true,
        "_VideoTags": tags,
    }))
    .unwrap()
}

fn tags() -> Value {
    json!([
        { "tag": { "name": "Karolynesyera" } },
        { "tag": { "name": "Other" } },
    ])
}

fn lambda(kind: LambdaKind, predicate: Expr) -> Expr {
    Expr::Lambda {
        kind,
        collection: vec!["_VideoTags".into()],
        var: "vt".into(),
        predicate: Box::new(predicate),
    }
}

fn tag_name(op: BinOp, name: &str) -> Expr {
    bin(
        op,
        Expr::Path(vec!["vt".into(), "tag".into(), "name".into()]),
        Expr::String(name.into()),
    )
}

#[test]
fn any_is_true_when_one_item_matches() {
    let e = lambda(LambdaKind::Any, tag_name(BinOp::Eq, "Karolynesyera"));
    assert_eq!(evaluate(&video_record(tags()), &e), json!(true));
}

#[test]
fn any_is_false_when_no_item_matches() {
    let e = lambda(LambdaKind::Any, tag_name(BinOp::Eq, "Nope"));
    assert_eq!(evaluate(&video_record(tags()), &e), json!(false));
}

#[test]
fn all_requires_every_item_to_match() {
    let e = lambda(LambdaKind::All, tag_name(BinOp::Eq, "Karolynesyera"));
    assert_eq!(evaluate(&video_record(tags()), &e), json!(false));
    let e = lambda(LambdaKind::All, tag_name(BinOp::Ne, "Nope"));
    assert_eq!(evaluate(&video_record(tags()), &e), json!(true));
}

#[test]
fn empty_collection_any_is_false_all_is_true() {
    let r = video_record(json!([]));
    let any = lambda(LambdaKind::Any, tag_name(BinOp::Eq, "x"));
    let all = lambda(LambdaKind::All, tag_name(BinOp::Eq, "x"));
    assert_eq!(evaluate(&r, &any), json!(false));
    assert_eq!(evaluate(&r, &all), json!(true));
}

#[test]
fn missing_collection_is_null() {
    let r = DataRecord::from_json(
        &json!({ "ID": "00000000-0000-0000-0000-000000000003", "IsActiveEntity": true }),
    )
    .unwrap();
    let e = lambda(LambdaKind::Any, tag_name(BinOp::Eq, "x"));
    assert_eq!(evaluate(&r, &e), Value::Null);
}

#[test]
fn unknown_item_results_make_any_null() {
    let r = video_record(json!([{ "other": 1 }]));
    let e = lambda(LambdaKind::Any, tag_name(BinOp::Gt, "a"));
    assert_eq!(evaluate(&r, &e), Value::Null);
}

#[test]
fn parsed_request_filter_evaluates() {
    let e = Expr::parse("_VideoTags/any(vt:vt/tag/name eq 'Karolynesyera')").unwrap();
    assert_eq!(evaluate(&video_record(tags()), &e), json!(true));
}
