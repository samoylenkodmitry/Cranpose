use super::shader_warm_ups;

#[test]
fn every_widget_shader_is_listed_once() {
    let warm_ups = shader_warm_ups();
    let identities: Vec<_> = warm_ups
        .iter()
        .map(|warm_up| {
            (
                warm_up.shader.source_hash(),
                warm_up.shader.overrides_hash(),
            )
        })
        .collect();
    assert_eq!(identities.len(), 3);
    for (index, identity) in identities.iter().enumerate() {
        assert!(
            !identities[..index].contains(identity),
            "a shader listed twice would compile twice"
        );
    }
}
