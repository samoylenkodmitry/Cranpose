use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};

use cranpose::{AppLauncher, GpuContext, gpu_context, prelude::*};

const SIZE: u32 = 8;

fn fill(gpu: &GpuContext, texture: &wgpu::Texture, color: wgpu::Color) {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    gpu.queue.submit([encoder.finish()]);
}

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
    let start = ((y * width + x) * 4) as usize;
    &pixels[start..start + 4]
}

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
    assert!(view.draw(&mut pixels).expect("first frame"));
    assert_eq!(pixel(&pixels, 64, 32, 24), &[255, 0, 0, 255]);
    assert_ne!(pixel(&pixels, 64, 2, 2), &[255, 0, 0, 255]);

    let engine = engine.get().expect("composed");
    fill(&engine.gpu, &engine.texture, wgpu::Color::BLUE);
    engine.frame.set(2);
    assert!(view.draw(&mut pixels).expect("second engine frame"));
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
