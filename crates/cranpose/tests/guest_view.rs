use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};

use cranpose::{AppLauncher, AppSettings, GpuContext, GuestView, gpu_context, prelude::*};

use crate::gpu_support::{draw_settled, draw_until_settled, fill, pixel, read_texture};

const SIZE: u32 = 8;

#[test]
fn an_external_texture_shows_each_frame_its_owner_draws() {
    struct Engine {
        gpu: GpuContext,
        texture: wgpu::Texture,
        frame: MutableState<u64>,
    }
    let engine: Rc<OnceCell<Engine>> = Rc::new(OnceCell::new());
    let shared = Rc::clone(&engine);
    let mut view = AppLauncher::new()
        .create_embedded_view(64, 48, 1.0, move || {
            let Some(gpu) = gpu_context() else {
                return;
            };
            let frame = rememberMutableStateOf(|| 1u64);
            let engine = shared.get_or_init(|| Engine {
                texture: gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("engine frame"),
                    size: wgpu::Extent3d {
                        width: SIZE,
                        height: SIZE,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                }),
                gpu,
                frame,
            });
            fill(&engine.gpu, &engine.texture, wgpu::Color::RED);
            let external = ExternalTexture::new(
                SIZE,
                SIZE,
                true,
                engine
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            )
            .expect("texture size");
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .padding(8.0)
                    .draw_behind(move |scope| {
                        scope.draw_image_at(
                            Rect::from_size(scope.size()),
                            external.frame(frame.value()),
                            1.0,
                            None,
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        })
        .expect("GPU embedded component");
    let mut pixels = Vec::new();
    assert!(draw_settled(&mut view, &mut pixels));
    assert_eq!(pixel(&pixels, 64, 32, 24), &[255, 0, 0, 255]);
    assert_ne!(pixel(&pixels, 64, 2, 2), &[255, 0, 0, 255]);

    let engine = engine.get().expect("composed");
    fill(&engine.gpu, &engine.texture, wgpu::Color::BLUE);
    engine.frame.set(2);
    assert!(draw_settled(&mut view, &mut pixels));
    assert_eq!(pixel(&pixels, 64, 32, 24), &[0, 0, 255, 255]);
}

#[test]
fn a_press_reports_whether_the_composition_took_it() {
    let clicks = Rc::new(Cell::new(0));
    let counter = Rc::clone(&clicks);
    let mut view = AppLauncher::new()
        .create_embedded_view(64, 48, 1.0, move || {
            let counter = Rc::clone(&counter);
            Box(
                Modifier::empty()
                    .size(Size::new(20.0, 20.0))
                    .clickable(move |_| counter.set(counter.get() + 1)),
                BoxSpec::default(),
                || {},
            );
        })
        .expect("GPU embedded component");
    let mut pixels = Vec::new();
    view.draw(&mut pixels).expect("first frame");

    let shell = view.shell();
    shell.set_cursor(10.0, 10.0);
    assert!(shell.pointer_pressed());
    shell.pointer_released();
    assert_eq!(clicks.get(), 1);

    shell.set_cursor(50.0, 40.0);
    assert!(!shell.pointer_pressed());
    shell.pointer_released();
    assert_eq!(clicks.get(), 1);
}

// A headset's compositor reads a panel's image as premultiplied alpha, so a panel with rounded
// corners and no background around them shows as that shape.
#[test]
fn a_rounded_box_on_a_transparent_texture_is_a_premultiplied_circle() {
    let held: Rc<OnceCell<GpuContext>> = Rc::new(OnceCell::new());
    let keep = Rc::clone(&held);
    let _host = AppLauncher::new()
        .create_embedded_view(16, 16, 1.0, move || {
            if let Some(gpu) = gpu_context() {
                keep.get_or_init(|| gpu);
            }
        })
        .expect("GPU embedded component");
    let gpu = held.get().expect("a GPU context").clone();
    // A circle of 40 dp at 1.5 pixels to the dp fills the texture
    let side = 60;
    let color = [1.0, 0.5, 0.0];
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut guest = GuestView::new(&AppSettings::default(), &gpu, format, side, side, 1.5, {
            move || {
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(color[0], color[1], color[2], 1.0))
                        .rounded_corners(20.0),
                    BoxSpec::default(),
                    || {},
                );
            }
        })
        .expect("guest view");
        guest.shell().renderer().set_transparent_background(true);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("circle"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        draw_until_settled(&mut guest, |guest| {
            guest.draw(&texture, &view).expect("frame");
            guest.shell().needs_redraw()
        });
        let pixels = read_texture(&gpu, &texture);
        assert_eq!(
            pixel(&pixels, side, 0, 0),
            &[0, 0, 0, 0],
            "{format:?}: outside the circle"
        );
        let middle = pixel(&pixels, side, 30, 30);
        assert_eq!(middle[3], 255, "{format:?}: inside the circle");
        // An edge pixel covers a part of itself. Its color is the circle's times that part,
        // whatever the format encodes. A straight color would be brighter.
        let srgb = format == wgpu::TextureFormat::Rgba8UnormSrgb;
        let edges = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|texel| texel[3] > 0 && texel[3] < 255)
            .collect::<Vec<_>>();
        assert!(
            !edges.is_empty(),
            "{format:?}: the circle has no edge pixels"
        );
        for edge in edges {
            for channel in 0..3 {
                let covered = linear(middle[channel], srgb) * f32::from(edge[3]) / 255.0;
                assert!(
                    (linear(edge[channel], srgb) - covered).abs() < 0.02,
                    "{format:?}: {edge:?} is not premultiplied against {middle:?}"
                );
            }
        }
    }
}

// The light that a stored channel byte stands for.
fn linear(byte: u8, srgb: bool) -> f32 {
    let value = f32::from(byte) / 255.0;
    if !srgb {
        value
    } else if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
