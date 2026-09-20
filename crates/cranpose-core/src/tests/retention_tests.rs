use super::*;

#[test]
fn retention_budget_default_is_unbounded() {
    assert_eq!(RetentionBudget::default(), RetentionBudget::UNBOUNDED);
    assert_eq!(RetentionBudget::default().max_retained_subtrees, None);
    assert_eq!(RetentionBudget::default().max_retained_bytes, None);
    assert_eq!(RetentionBudget::default().max_age_passes, None);
}

#[test]
fn retention_policy_default_uses_unbounded_budget_and_detach_lru() {
    assert_eq!(RetentionPolicy::default(), RetentionPolicy::UNBOUNDED);
    assert_eq!(
        RetentionPolicy::default().budget,
        RetentionBudget::UNBOUNDED
    );
    assert_eq!(
        RetentionPolicy::default().eviction,
        RetentionEvictionPolicy::LeastRecentlyDetached
    );
}

#[test]
fn retention_budget_can_express_all_limits() {
    let budget = RetentionBudget {
        max_retained_subtrees: Some(3),
        max_retained_bytes: Some(4096),
        max_age_passes: Some(5),
    };

    assert_eq!(budget.max_retained_subtrees, Some(3));
    assert_eq!(budget.max_retained_bytes, Some(4096));
    assert_eq!(budget.max_age_passes, Some(5));
}
