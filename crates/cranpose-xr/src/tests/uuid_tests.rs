use super::*;

const TEXT: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn an_identifier_prints_and_reads_back_in_the_form_of_java() {
    let uuid: Uuid = TEXT.parse().unwrap();
    assert_eq!(uuid.to_string(), TEXT);
    assert_eq!(uuid.as_bytes()[0], 0x12);
    assert_eq!(uuid.as_bytes()[15], 0x00);
    assert_eq!(Uuid::from_bytes(*uuid.as_bytes()), uuid);
}

#[test]
fn capital_digits_read_as_their_lower_case_value() {
    let upper: Uuid = TEXT.to_uppercase().parse().unwrap();
    assert_eq!(upper, TEXT.parse().unwrap());
}

#[test]
fn text_that_is_not_an_identifier_is_refused() {
    for text in [
        "",
        "123e4567e89b12d3a456426614174000",
        "123e4567-e89b-12d3-a456-42661417400",
        "123e4567-e89b-12d3-a456-4266141740000",
        "123e4567-e89b-12d3-a456-42661417400g",
        "123e4567_e89b-12d3-a456-426614174000",
        "-123e4567-e89b-12d3-a456-426614174000",
    ] {
        assert_eq!(text.parse::<Uuid>(), Err(UuidParseError), "{text:?}");
    }
}

#[test]
fn the_zero_identifier_prints_with_every_digit() {
    assert_eq!(
        Uuid::from_bytes([0; 16]).to_string(),
        "00000000-0000-0000-0000-000000000000"
    );
}
