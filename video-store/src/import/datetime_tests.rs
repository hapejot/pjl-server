use super::*;

fn norm(s: &str) -> Value {
    datetime_offset(Value::String(s.into()))
}

#[test]
fn microseconds_are_cut_to_millis() {
    assert_eq!(
        norm("2026-05-30T16:52:22.271179+02:00"),
        Value::String("2026-05-30T16:52:22.271+02:00".into())
    );
}

#[test]
fn short_fraction_is_padded() {
    assert_eq!(
        norm("2026-05-30T16:52:22.5+02:00"),
        Value::String("2026-05-30T16:52:22.500+02:00".into())
    );
}

#[test]
fn short_offset_is_extended() {
    assert_eq!(
        norm("2026-05-30T16:52:22.271179+02"),
        Value::String("2026-05-30T16:52:22.271+02:00".into())
    );
}

#[test]
fn utc_and_no_fraction_are_kept() {
    assert_eq!(
        norm("2026-05-30T16:52:22Z"),
        Value::String("2026-05-30T16:52:22Z".into())
    );
    assert_eq!(
        norm("2026-05-30T16:52:22+01:00"),
        Value::String("2026-05-30T16:52:22+01:00".into())
    );
}

#[test]
fn empty_becomes_null() {
    assert_eq!(norm(""), Value::Null);
    assert_eq!(norm("   "), Value::Null);
    assert_eq!(datetime_offset(Value::Null), Value::Null);
}
