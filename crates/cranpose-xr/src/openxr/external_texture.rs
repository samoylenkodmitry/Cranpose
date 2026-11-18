//! Draws a decoder's hardware buffer into the swapchain images of a projection layer.
//!
//! The buffer becomes an EGL image, and the EGL image an external texture. A small shader
//! samples it through `samplerExternalOES`, which also turns the decoder's YUV into RGB. The
//! pass runs on the session's GL context beside wgpu, which tracks its GL state for one command
//! buffer at a time and resets the part that it relies on at the start of each. The pass sets
//! what it depends on and restores the vertex array that wgpu leaves bound.
#![expect(
    unsafe_code,
    reason = "GL and EGL calls, and function pointers that eglGetProcAddress returns"
)]

#[cfg(feature = "stream-costs")]
use std::time::Duration;
use std::{ffi::c_void, mem, num::NonZeroU32};

use glow::HasContext;

use super::graphics::Graphics;
use crate::foveated_unwarp::FoveatedUnwarp;

// `EGL_NATIVE_BUFFER_ANDROID`: the target of an EGL image made from an `AHardwareBuffer`.
const EGL_NATIVE_BUFFER_ANDROID: u32 = 0x3140;
// `GL_TEXTURE_EXTERNAL_OES`
const TEXTURE_EXTERNAL_OES: u32 = 0x8D65;
// The picture's first row is its top. The compositor reads the first row of a swapchain image as
// the bottom one, so each pass puts the buffer's first row at the target's last row.
const FLIP_ROWS: bool = true;
// The queries in flight at once, so that a result is read some frames after its pass.
#[cfg(feature = "stream-costs")]
const TIMERS: usize = 3;

type CreateImage = unsafe extern "C" fn(
    display: *mut c_void,
    context: *mut c_void,
    target: u32,
    buffer: *mut c_void,
    attributes: *const i32,
) -> *mut c_void;
type DestroyImage = unsafe extern "C" fn(display: *mut c_void, image: *mut c_void) -> u32;
type NativeClientBuffer = unsafe extern "C" fn(buffer: *const c_void) -> *mut c_void;
type TargetTexture = unsafe extern "C" fn(target: u32, image: *mut c_void);

// The corner of the target as a coordinate: `origin + corner * step`. The plain shader reads the
// picture there. The foveated shader reads the point of the eye's full view, which its unwarp
// turns into a point of the picture.
const VERTEX: &str = r"#version 300 es
uniform vec2 origin;
uniform vec2 step;
out vec2 uv;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
    uv = origin + corner * step;
}
";

const FRAGMENT_HEAD: &str = r"#version 300 es
#extension GL_OES_EGL_image_external_essl3 : require
precision highp float;
uniform samplerExternalOES picture;
in vec2 uv;
out vec4 color;
// The picture's values are encoded like an sRGB picture. The swapchain holds linear light:
// the compositor reads a plain format as linear, and an sRGB format encodes what the pass
// writes.
vec3 linear(vec3 encoded) {
    return mix(encoded / 12.92, pow((encoded + 0.055) / 1.055, vec3(2.4)), step(0.04045, encoded));
}
";

const FRAGMENT_PLAIN: &str = r"
void main() {
    color = vec4(linear(texture(picture, uv).rgb), 1.0);
}
";

// The unwarp of a foveated picture: `FoveatedUnwarp::sample` holds the same formulas. `part` is
// the picture's rectangle in the buffer, as fractions of the buffer from its top left corner,
// and `unwarp` holds the constants of both axes, which `FoveatedUnwarp::axes` packs.
const FRAGMENT_FOVEATED: &str = r"
uniform vec4 part;
uniform vec4 unwarp[6];
uniform float mirror;
float unwarp_axis(float view, vec4 bounds, vec4 lower, vec4 upper) {
    float position;
    if (view < bounds.x) {
        position = (sqrt(max(lower.y * lower.y + 4.0 * lower.x * view, 0.0)) - lower.y) / (2.0 * lower.x);
    } else if (view > bounds.y) {
        position = (sqrt(max(upper.y * upper.y - 4.0 * (upper.z - upper.x * view), 0.0)) - upper.y) / (2.0 * upper.x);
    } else {
        position = (view - bounds.z) * bounds.w;
    }
    return position * lower.z;
}
void main() {
    float x = mix(uv.x, 1.0 - uv.x, mirror);
    vec2 encoded = vec2(
        unwarp_axis(x, unwarp[0], unwarp[1], unwarp[2]),
        unwarp_axis(uv.y, unwarp[3], unwarp[4], unwarp[5])
    );
    encoded.x = mix(encoded.x, 1.0 - encoded.x, mirror);
    color = vec4(linear(texture(picture, part.xy + encoded * part.zw).rgb), 1.0);
}
";

// One draw of a part of a hardware buffer over a whole target.
pub(crate) struct Pass<'a> {
    pub(crate) target: &'a Target,
    // The target's size in pixels
    pub(crate) size: (u32, u32),
    // The part: left, top, width and height in pixels from the buffer's top left corner
    pub(crate) part: [u32; 4],
    // How to undo the compression of a foveated part, or `None` for a part that has the same
    // density everywhere. The part fills the target either way.
    pub(crate) unwarp: Option<&'a FoveatedUnwarp>,
}

// A texture that a pass draws into, with the framebuffer that holds it.
pub(crate) struct Target(glow::Framebuffer);

pub(crate) struct ExternalTexture {
    gl: glow::Context,
    display: *mut c_void,
    create_image: CreateImage,
    destroy_image: DestroyImage,
    native_client_buffer: NativeClientBuffer,
    target_texture: TargetTexture,
    plain: Shader,
    // Linked when the first foveated pass needs it, so that an app with no foveated picture pays
    // nothing for it
    foveated: Option<Shader>,
    texture: glow::Texture,
    array: glow::VertexArray,
    #[cfg(feature = "stream-costs")]
    timers: Option<Timers>,
}

// A linked program with the locations of its uniforms.
struct Shader {
    program: glow::Program,
    origin: Option<glow::UniformLocation>,
    step: Option<glow::UniformLocation>,
    // The uniforms of the foveated unwarp, which only that program has
    part: Option<glow::UniformLocation>,
    unwarp: Option<glow::UniformLocation>,
    mirror: Option<glow::UniformLocation>,
}

// Queries of the GPU time that the passes take, read without waiting.
#[cfg(feature = "stream-costs")]
struct Timers {
    queries: [glow::Query; TIMERS],
    // The next query to begin, and how many are in flight
    next: usize,
    pending: usize,
}

impl ExternalTexture {
    // Makes the shader and the external texture on the current context. A missing function names
    // itself in the error.
    pub(crate) fn new(context: &Graphics) -> Result<Self, String> {
        let function = |name: &str| {
            context
                .proc_address(name)
                .ok_or_else(|| format!("the driver has no {name}"))
        };
        // SAFETY: each pointer is the function that eglGetProcAddress returned for the name,
        // whose signature the EGL and GL extension specifications give.
        let (create_image, destroy_image, native_client_buffer, target_texture) = unsafe {
            (
                mem::transmute::<*const c_void, CreateImage>(function("eglCreateImageKHR")?),
                mem::transmute::<*const c_void, DestroyImage>(function("eglDestroyImageKHR")?),
                mem::transmute::<*const c_void, NativeClientBuffer>(function(
                    "eglGetNativeClientBufferANDROID",
                )?),
                mem::transmute::<*const c_void, TargetTexture>(function(
                    "glEGLImageTargetTexture2DOES",
                )?),
            )
        };
        // SAFETY: the loader returns the addresses of the current context's functions.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                context.proc_address(name).unwrap_or(std::ptr::null())
            })
        };
        if !gl
            .supported_extensions()
            .contains("GL_OES_EGL_image_external_essl3")
        {
            return Err("the driver has no GL_OES_EGL_image_external_essl3".into());
        }
        let display = context.display();
        // SAFETY: the context is current on this thread.
        unsafe {
            let plain = Shader::new(&gl, FRAGMENT_PLAIN)?;
            let texture = gl.create_texture()?;
            gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(texture));
            for (parameter, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(TEXTURE_EXTERNAL_OES, parameter, value as i32);
            }
            gl.bind_texture(TEXTURE_EXTERNAL_OES, None);
            let array = gl.create_vertex_array()?;
            Ok(Self {
                gl,
                display,
                create_image,
                destroy_image,
                native_client_buffer,
                target_texture,
                plain,
                foveated: None,
                texture,
                array,
                #[cfg(feature = "stream-costs")]
                timers: None,
            })
        }
    }

    // Makes the target that draws into the GL texture `name`.
    pub(crate) fn target(&self, name: u32) -> Result<Target, String> {
        let name = NonZeroU32::new(name).ok_or("a swapchain image with no GL name")?;
        // SAFETY: the context is current, and the texture is a live 2D texture of the session.
        unsafe {
            let gl = &self.gl;
            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(glow::NativeTexture(name)),
                0,
            );
            let complete = gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            if complete != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(framebuffer);
                return Err(format!("the swapchain image's framebuffer: {complete:#x}"));
            }
            Ok(Target(framebuffer))
        }
    }

    pub(crate) fn delete_target(&self, target: Target) {
        // SAFETY: the context is current, and the framebuffer is live and owned by the target.
        unsafe { self.gl.delete_framebuffer(target.0) };
    }

    // Draws the parts of `buffer`, which is `size` pixels, that `passes` name. The buffer becomes
    // one EGL image for all the passes.
    pub(crate) fn draw(
        &mut self,
        buffer: *mut c_void,
        size: (u32, u32),
        passes: &[Pass<'_>],
    ) -> Result<(), String> {
        if self.foveated.is_none() && passes.iter().any(|pass| pass.unwarp.is_some()) {
            let started = std::time::Instant::now();
            // SAFETY: the context is current on this thread.
            self.foveated = Some(unsafe { Shader::new(&self.gl, FRAGMENT_FOVEATED)? });
            log::info!(
                "cranpose xr: the foveated stream shader compiled in {:?}",
                started.elapsed()
            );
        }
        let gl = &self.gl;
        // SAFETY: the buffer is a live AHardwareBuffer that the frame holds, and the function
        // pointers match their extensions.
        let client = unsafe { (self.native_client_buffer)(buffer) };
        if client.is_null() {
            return Err("the buffer has no EGL client buffer".into());
        }
        // SAFETY: as above, and the display is the session's.
        let image = unsafe {
            (self.create_image)(
                self.display,
                std::ptr::null_mut(),
                EGL_NATIVE_BUFFER_ANDROID,
                client,
                std::ptr::null(),
            )
        };
        if image.is_null() {
            return Err("the buffer made no EGL image".into());
        }
        let (buffer_width, buffer_height) = (size.0 as f32, size.1 as f32);
        // SAFETY: the context is current, and every object belongs to it.
        unsafe {
            let array = gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING);
            let unit = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(self.texture));
            (self.target_texture)(TEXTURE_EXTERNAL_OES, image);
            // A sampler object that wgpu left on the unit would replace the texture's own
            gl.bind_sampler(0, None);
            for state in [
                glow::BLEND,
                glow::SCISSOR_TEST,
                glow::DEPTH_TEST,
                glow::STENCIL_TEST,
                glow::CULL_FACE,
            ] {
                gl.disable(state);
            }
            gl.color_mask(true, true, true, true);
            gl.bind_vertex_array(Some(self.array));
            for pass in passes {
                let [left, top, width, height] = pass.part.map(|pixels| pixels as f32);
                let part = [
                    left / buffer_width,
                    top / buffer_height,
                    width / buffer_width,
                    height / buffer_height,
                ];
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(pass.target.0));
                gl.viewport(0, 0, pass.size.0 as i32, pass.size.1 as i32);
                // The shader maps a target's first row to the lowest value of the vertical
                // coordinate. The buffer's first row is the picture's top at a coordinate of
                // zero.
                match (pass.unwarp, &self.foveated) {
                    (Some(unwarp), Some(foveated)) => {
                        gl.use_program(Some(foveated.program));
                        // The view's coordinate runs down from its top, which is the target's
                        // last row
                        let (y, rise) = if FLIP_ROWS { (1.0, -1.0) } else { (0.0, 1.0) };
                        gl.uniform_2_f32(foveated.origin.as_ref(), 0.0, y);
                        gl.uniform_2_f32(foveated.step.as_ref(), 1.0, rise);
                        gl.uniform_4_f32_slice(foveated.part.as_ref(), &part);
                        gl.uniform_4_f32_slice(foveated.unwarp.as_ref(), &unwarp.axes());
                        gl.uniform_1_f32(foveated.mirror.as_ref(), unwarp.mirror());
                    }
                    _ => {
                        gl.use_program(Some(self.plain.program));
                        let [left, top, width, height] = part;
                        let (y, rise) = if FLIP_ROWS {
                            (top + height, -height)
                        } else {
                            (top, height)
                        };
                        gl.uniform_2_f32(self.plain.origin.as_ref(), left, y);
                        gl.uniform_2_f32(self.plain.step.as_ref(), width, rise);
                    }
                }
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            }
            gl.bind_vertex_array(NonZeroU32::new(array as u32).map(glow::NativeVertexArray));
            gl.use_program(None);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            gl.bind_texture(TEXTURE_EXTERNAL_OES, None);
            gl.active_texture(unit as u32);
            (self.destroy_image)(self.display, image);
        }
        Ok(())
    }

    #[cfg(feature = "stream-costs")]
    // Times the GPU work of the passes, if the driver can.
    pub(crate) fn enable_timing(&mut self) {
        self.timers = Timers::new(&self.gl);
    }

    #[cfg(feature = "stream-costs")]
    // Starts timing the GPU time of the passes that follow.
    pub(crate) fn begin_timer(&mut self) {
        let Some(timers) = &mut self.timers else {
            return;
        };
        if timers.pending == TIMERS {
            return;
        }
        // SAFETY: the context is current, and the query is live and idle.
        unsafe {
            self.gl
                .begin_query(glow::TIME_ELAPSED, timers.queries[timers.next]);
        }
    }

    #[cfg(feature = "stream-costs")]
    // Stops the timing that `begin_timer` began.
    pub(crate) fn end_timer(&mut self) {
        let Some(timers) = &mut self.timers else {
            return;
        };
        if timers.pending == TIMERS {
            return;
        }
        // SAFETY: the context is current, and a timed query is open.
        unsafe { self.gl.end_query(glow::TIME_ELAPSED) };
        timers.next = (timers.next + 1) % TIMERS;
        timers.pending += 1;
    }

    #[cfg(feature = "stream-costs")]
    // The GPU time of the oldest timed passes, if their result is ready. It never waits.
    pub(crate) fn gpu_time(&mut self) -> Option<Duration> {
        let timers = self.timers.as_mut()?;
        if timers.pending == 0 {
            return None;
        }
        let oldest = (timers.next + TIMERS - timers.pending) % TIMERS;
        let query = timers.queries[oldest];
        // SAFETY: the context is current, and the query ended.
        unsafe {
            if self
                .gl
                .get_query_parameter_u32(query, glow::QUERY_RESULT_AVAILABLE)
                == 0
            {
                return None;
            }
            let disjoint = self.gl.get_parameter_i32(GPU_DISJOINT) != 0;
            let nanoseconds = self.gl.get_query_parameter_u32(query, glow::QUERY_RESULT);
            timers.pending -= 1;
            (!disjoint).then(|| Duration::from_nanos(u64::from(nanoseconds)))
        }
    }
}

// `GL_GPU_DISJOINT_EXT`: set when a timer result is not reliable.
#[cfg(feature = "stream-costs")]
const GPU_DISJOINT: u32 = 0x8FBB;

#[cfg(feature = "stream-costs")]
impl Timers {
    fn new(gl: &glow::Context) -> Option<Self> {
        if !gl
            .supported_extensions()
            .contains("GL_EXT_disjoint_timer_query")
        {
            return None;
        }
        // SAFETY: the context is current.
        let queries = unsafe {
            [
                gl.create_query().ok()?,
                gl.create_query().ok()?,
                gl.create_query().ok()?,
            ]
        };
        Some(Self {
            queries,
            next: 0,
            pending: 0,
        })
    }
}

impl Drop for ExternalTexture {
    fn drop(&mut self) {
        // SAFETY: the context is current on this thread until the session's end, and every
        // object is live and owned here.
        unsafe {
            #[cfg(feature = "stream-costs")]
            if let Some(timers) = self.timers.take() {
                for query in timers.queries {
                    self.gl.delete_query(query);
                }
            }
            self.gl.delete_vertex_array(self.array);
            self.gl.delete_texture(self.texture);
            self.gl.delete_program(self.plain.program);
            if let Some(foveated) = &self.foveated {
                self.gl.delete_program(foveated.program);
            }
        }
    }
}

impl Shader {
    // Compiles and links the vertex shader with the fragment shader that `main` completes.
    //
    // # Safety
    //
    // The caller has the context current.
    unsafe fn new(gl: &glow::Context, main: &str) -> Result<Self, String> {
        let fragment = [FRAGMENT_HEAD, main].concat();
        // SAFETY: the caller has the context current.
        unsafe {
            let program = link(gl, &fragment)?;
            let location = |name: &str| gl.get_uniform_location(program, name);
            Ok(Self {
                program,
                origin: location("origin"),
                step: location("step"),
                part: location("part"),
                unwarp: location("unwarp"),
                mirror: location("mirror"),
            })
        }
    }
}

// Compiles and links the shader program from the fragment shader's source.
unsafe fn link(gl: &glow::Context, fragment: &str) -> Result<glow::Program, String> {
    // SAFETY: the caller has the context current.
    unsafe {
        let program = gl.create_program()?;
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
                return Err(format!("the stream shader: {log}"));
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("the stream program: {log}"));
        }
        Ok(program)
    }
}
