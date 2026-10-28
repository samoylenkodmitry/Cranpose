use super::*;

const COLORS: PerfectMap<u32, 6> = PerfectMap::new([
    ("red", 0xff0000),
    ("green", 0x00ff00),
    ("blue", 0x0000ff),
    ("a name longer than one lane", 1),
    ("a name longer than two lanes of bytes", 2),
    ("", 3),
]);

#[test]
fn every_key_finds_its_value() {
    assert_eq!(COLORS.get("red"), Some(&0xff0000));
    assert_eq!(COLORS.get("green"), Some(&0x00ff00));
    assert_eq!(COLORS.get("blue"), Some(&0x0000ff));
    assert_eq!(COLORS.get("a name longer than one lane"), Some(&1));
    assert_eq!(
        COLORS.get("a name longer than two lanes of bytes"),
        Some(&2)
    );
    assert_eq!(COLORS.get(""), Some(&3));
}

#[test]
fn a_string_that_is_not_a_key_finds_nothing() {
    for miss in [
        "Red",
        "re",
        "redd",
        "blue ",
        "a name longer than one lan",
        "x",
    ] {
        assert_eq!(COLORS.get(miss), None, "{miss:?}");
    }
}

#[test]
fn the_most_keys_a_map_holds_all_find_their_values() {
    let keys: Vec<&'static str> = (0..MAX_KEYS)
        .map(|index| &*Box::leak(format!("key {index}").into_boxed_str()))
        .collect();
    let map = PerfectMap::new(std::array::from_fn::<_, MAX_KEYS, _>(|index| {
        (keys[index], index)
    }));
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(map.get(key), Some(&index));
    }
    assert_eq!(map.get(&format!("key {MAX_KEYS}")), None);
}

#[test]
#[should_panic(expected = "no seed places these keys")]
fn a_key_given_twice_is_refused() {
    let _ = PerfectMap::new([("one", 1), ("two", 2), ("one", 3)]);
}
