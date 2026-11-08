use super::*;

#[test]
fn datetime_offset_literal_parses_as_one_token() {
    let input =
        "name eq 'x' and created_at eq 2026-03-21T23:43:13.8630000Z and IsActiveEntity eq true";
    let expr = parse_filter(input).expect("Filter mit Zeitliteral parst");
    let text = format!("{expr:?}");
    assert!(
        text.contains("DateTime(\"2026-03-21T23:43:13.8630000Z\")"),
        "{text}"
    );
}

#[test]
fn plain_date_and_negative_numbers_still_work() {
    assert!(parse_filter("due eq 2026-03-21").is_ok());
    assert!(parse_filter("Amount gt -3").is_ok());
}

#[test]
fn empty_value_at_end_is_null() {
    let expr = parse_filter("ID eq ").expect("leerer Wert parst");
    assert!(format!("{expr:?}").contains("Null"), "{expr:?}");
}
