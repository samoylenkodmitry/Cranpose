use super::*;

#[test]
fn test_find_word_start() {
    assert_eq!(find_word_start("hello world", 6), 0);
    assert_eq!(find_word_start("hello world", 11), 6);
    assert_eq!(find_word_start("hello", 0), 0);
}

#[test]
fn test_find_word_end() {
    assert_eq!(find_word_end("hello world", 0), 5);
    assert_eq!(find_word_end("hello world", 6), 11);
}

#[test]
fn test_find_word_boundaries() {
    let (start, end) = find_word_boundaries("hello world", 2);
    assert_eq!(start, 0);
    assert_eq!(end, 5);

    let (start, end) = find_word_boundaries("hello world", 8);
    assert_eq!(start, 6);
    assert_eq!(end, 11);
}
