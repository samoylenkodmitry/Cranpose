use super::*;

fn selection() -> InkSelection {
    InkSelection {
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        color: Color::WHITE,
        content_zoom: 1.0,
        activity: 0.0,
        optical_scale: 1.0,
        projection: Size {
            width: 10.0,
            height: 10.0,
        },
    }
}

fn grid() -> InkGrid {
    InkGrid {
        first_center: (5.0, 5.0),
        pitch: 10.0,
        count: 1,
    }
}

#[test]
fn each_warm_up_names_the_pipeline_its_pass_draws() {
    let warm_ups = shader_warm_ups();
    let size = Size {
        width: 10.0,
        height: 10.0,
    };
    for (warm_up, pass, target) in [
        (&warm_ups[0], InkPass::Color, ShaderTarget::Page),
        (&warm_ups[1], InkPass::Mask, ShaderTarget::Layer),
    ] {
        let RenderEffect::Shader { shader } = effect(pass, size, selection(), grid(), false) else {
            panic!("vibrancy is a runtime shader effect");
        };
        assert_eq!(warm_up.shader.source_hash(), shader.source_hash());
        assert_eq!(warm_up.shader.overrides_hash(), shader.overrides_hash());
        assert_eq!(warm_up.target, target);
    }
    assert_ne!(
        warm_ups[0].shader.overrides_hash(),
        warm_ups[1].shader.overrides_hash(),
        "the mask override is what tells the two pipelines apart"
    );
}
