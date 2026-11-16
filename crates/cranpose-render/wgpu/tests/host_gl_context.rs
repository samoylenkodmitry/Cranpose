// A host that makes a GL context and keeps it current on its own thread, as an OpenXR app does
// with its session's context, hands the renderer a device whose context wgpu does not own.
#![expect(unsafe_code, reason = "makes an EGL context and hands it to wgpu")]

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use cranpose_render_common::graph::{DrawRunNode, PrimitivePhase, RenderNode};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect};
use khronos_egl as egl;

use crate::support::{self, LockedRenderer};

const SIZE: u32 = 64;
// Mesa's display without a window system, which a test box without a display server has
const PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;

// An EGL context of the test's own, current on the test's thread.
struct HostContext {
    egl: egl::DynamicInstance<egl::EGL1_5>,
}

impl HostContext {
    fn make_current() -> Result<Self, String> {
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required() }
            .map_err(|error| format!("libEGL: {error}"))?;
        let display = unsafe {
            egl.get_platform_display(
                PLATFORM_SURFACELESS_MESA,
                egl::DEFAULT_DISPLAY,
                &[egl::ATTRIB_NONE],
            )
        }
        .map_err(|error| format!("EGL display: {error}"))?;
        egl.initialize(display)
            .map_err(|error| format!("EGL: {error}"))?;
        egl.bind_api(egl::OPENGL_ES_API)
            .map_err(|error| format!("EGL: {error}"))?;
        let config = egl
            .choose_first_config(
                display,
                &[
                    egl::RENDERABLE_TYPE,
                    egl::OPENGL_ES3_BIT,
                    egl::SURFACE_TYPE,
                    egl::PBUFFER_BIT,
                    egl::NONE,
                ],
            )
            .map_err(|error| format!("EGL: {error}"))?
            .ok_or("no GLES 3 config")?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            )
            .map_err(|error| format!("EGL context: {error}"))?;
        let surface = egl
            .create_pbuffer_surface(
                display,
                config,
                &[egl::WIDTH, 16, egl::HEIGHT, 16, egl::NONE],
            )
            .map_err(|error| format!("EGL surface: {error}"))?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .map_err(|error| format!("EGL: {error}"))?;
        Ok(Self { egl })
    }

    fn adapter(&self) -> Result<wgpu::Adapter, String> {
        let lookup = |name: &str| {
            self.egl
                .get_proc_address(name)
                .map_or(std::ptr::null(), |function| function as *const c_void)
        };
        let exposed = unsafe {
            wgpu::hal::gles::Adapter::new_external(lookup, wgpu::GlBackendOptions::default())
        }
        .ok_or("no GL adapter on the host's context")?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::GL;
        Ok(unsafe { wgpu::Instance::new(descriptor).create_adapter_from_hal(exposed) })
    }
}

#[test]
fn a_host_gl_context_draws_its_first_frame_whole() {
    let host = match HostContext::make_current() {
        Ok(host) => host,
        Err(error) => panic!("this test needs EGL with GLES 3: {error}"),
    };
    // A compiler thread that calls GL with no context current fails and ends
    let compiler_failures = Arc::new(AtomicUsize::new(0));
    let failures = Arc::clone(&compiler_failures);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current()
            .name()
            .is_some_and(|name| name.starts_with("cranpose-"))
        {
            failures.fetch_add(1, Ordering::Relaxed);
        }
        previous(info);
    }));
    let adapter = host.adapter().expect("GL adapter");
    let mut renderer = LockedRenderer::on_adapter(&adapter).expect("renderer");
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    let graph = support::page_graph(
        SIZE,
        SIZE,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![DrawPrimitive::Rect {
                rect: bounds,
                brush: Brush::solid(Color::from_rgba_u8(220, 30, 40, 255)),
                stroke: None,
            }],
        ))],
    );
    // The background compiler's threads have no current context: a frame that waited for them
    // would show stand-ins.
    let frame = support::capture_graph(&mut renderer, &graph, SIZE, SIZE);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(support::pipelines_settled(&stats), "{stats:?}");
    let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let pixel = &frame.pixels[middle..middle + 4];
    assert!(pixel[0] > 200 && pixel[1] < 60, "{pixel:?}");
    support::wait_for_background_compiler_idle();
    assert_eq!(compiler_failures.load(Ordering::Relaxed), 0);
}
