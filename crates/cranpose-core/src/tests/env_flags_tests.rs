#[test]
fn an_unset_switch_reads_false_and_stays_false() {
    assert!(!env_flag!("CRANPOSE_ENV_FLAG_THAT_NOBODY_SETS"));
    assert!(!env_flag!("CRANPOSE_ENV_FLAG_THAT_NOBODY_SETS"));
}

#[test]
fn an_unset_threshold_reads_none() {
    assert_eq!(env_threshold_ms!("CRANPOSE_ENV_MS_THAT_NOBODY_SETS"), None);
}

#[test]
fn each_call_site_caches_the_value_it_read_first() {
    let first = env_flag!("CRANPOSE_ENV_FLAG_CACHE_PROBE");
    let second = env_flag!("CRANPOSE_ENV_FLAG_CACHE_PROBE");
    assert_eq!(first, second);
}
