use super::MetaEntitlement;

#[test]
fn the_javas_numbers_name_the_states_of_the_check() {
    let named = [0, 1, 2, 3].map(MetaEntitlement::from_state);
    assert_eq!(
        named,
        [
            MetaEntitlement::NotStarted,
            MetaEntitlement::Checking,
            MetaEntitlement::Entitled,
            MetaEntitlement::NotEntitled,
        ]
    );
}

#[test]
fn a_number_that_the_java_does_not_give_means_the_check_has_not_started() {
    for state in [-1, 4, i32::MAX, i32::MIN] {
        assert_eq!(
            MetaEntitlement::from_state(state),
            MetaEntitlement::NotStarted
        );
    }
}
