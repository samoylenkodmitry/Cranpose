use cranpose::{AppLauncher, GuestView};
use cranpose_gpu_interop::godot::{GodotGpu, GodotTexture};
use godot::{
    classes::{
        CanvasItemMaterial, CanvasLayer, DisplayServer, InputEvent, InputEventMouseButton,
        InputEventMouseMotion, RdTextureFormat, RdTextureView, RenderingDevice, RenderingServer,
        Texture2Drd, TextureRect,
        canvas_item_material::BlendMode,
        control::{LayoutPreset, MouseFilter},
        rendering_device::{DataFormat, TextureUsageBits},
        texture_rect::{ExpandMode, StretchMode},
    },
    global::MouseButton,
    prelude::*,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const WHEEL_STEP: f32 = 40.0;

struct Output {
    rid: Rid,
    godot: GodotTexture,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub struct Surface {
    gpu: GodotGpu,
    rd: Gd<RenderingDevice>,
    view: GuestView,
    output: Option<Output>,
    display: Gd<Texture2Drd>,
    density: f32,
    pressed: bool,
}

impl Surface {
    pub fn new(
        layer: &mut Gd<CanvasLayer>,
        transparent: bool,
        content: impl FnMut() + 'static,
    ) -> Result<Self, String> {
        let mut rd = RenderingServer::singleton()
            .get_rendering_device()
            .ok_or("Godot renders without a RenderingDevice")?;
        let gpu = GodotGpu::new(&mut rd).map_err(|error| error.to_string())?;
        let density = DisplayServer::singleton().screen_get_scale();
        let mut view = AppLauncher::new()
            .create_guest_view(gpu.context(), FORMAT, 1, 1, density, content)
            .map_err(|error| error.to_string())?;
        view.shell()
            .renderer()
            .set_transparent_background(transparent);
        let display = Texture2Drd::new_gd();
        let mut rect = TextureRect::new_alloc();
        rect.set_anchors_preset(LayoutPreset::FULL_RECT);
        rect.set_expand_mode(ExpandMode::IGNORE_SIZE);
        rect.set_stretch_mode(StretchMode::SCALE);
        rect.set_mouse_filter(MouseFilter::IGNORE);
        rect.set_texture(&display);
        if transparent {
            let mut material = CanvasItemMaterial::new_gd();
            material.set_blend_mode(BlendMode::PREMULT_ALPHA);
            rect.set_material(&material);
        }
        layer.add_child(&rect);
        Ok(Self {
            gpu,
            rd,
            view,
            output: None,
            display,
            density,
            pressed: false,
        })
    }

    pub fn gpu(&self) -> &GodotGpu {
        &self.gpu
    }

    pub fn godot_texture(&mut self, rid: Rid) -> Result<GodotTexture, String> {
        self.gpu
            .texture(&mut self.rd, rid)
            .map_err(|error| error.to_string())
    }

    pub fn draw(&mut self, size: Vector2i) -> Result<(), String> {
        let (width, height) = (size.x.max(1) as u32, size.y.max(1) as u32);
        let resized = self
            .view
            .resize(width, height, self.density)
            .map_err(|error| error.to_string())?;
        if resized || self.output.is_none() {
            self.replace_output(width, height)?;
            self.view.shell().mark_dirty();
        }
        let Some(output) = self.output.as_ref() else {
            return Ok(());
        };
        let ticket = self.gpu.begin_frame();
        let drawn = self.view.draw(&output.texture, &output.view);
        self.gpu.finish_frame(
            ticket,
            drawn.as_ref().is_ok_and(|drew| *drew),
            &output.texture,
            &output.godot,
        );
        drawn.map(|_| ()).map_err(|error| error.to_string())
    }

    pub fn input(&mut self, event: &Gd<InputEvent>) -> bool {
        let shell = self.view.shell();
        if let Ok(motion) = event.clone().try_cast::<InputEventMouseMotion>() {
            let position = motion.get_position() / self.density;
            shell.set_cursor(position.x, position.y);
            return self.pressed;
        }
        let Ok(button) = event.clone().try_cast::<InputEventMouseButton>() else {
            return false;
        };
        let position = button.get_position() / self.density;
        shell.set_cursor(position.x, position.y);
        match button.get_button_index() {
            MouseButton::LEFT if button.is_pressed() => {
                self.pressed = shell.pointer_pressed();
                self.pressed
            }
            MouseButton::LEFT => {
                shell.pointer_released();
                std::mem::take(&mut self.pressed)
            }
            MouseButton::WHEEL_UP if button.is_pressed() => {
                shell.pointer_scrolled(0.0, -WHEEL_STEP)
            }
            MouseButton::WHEEL_DOWN if button.is_pressed() => {
                shell.pointer_scrolled(0.0, WHEEL_STEP)
            }
            _ => false,
        }
    }

    fn replace_output(&mut self, width: u32, height: u32) -> Result<(), String> {
        let mut format = RdTextureFormat::new_gd();
        format.set_width(width);
        format.set_height(height);
        format.set_format(DataFormat::R8G8B8A8_UNORM);
        format.set_usage_bits(
            TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::COLOR_ATTACHMENT_BIT
                | TextureUsageBits::CAN_COPY_TO_BIT,
        );
        let rid = self.rd.texture_create(&format, &RdTextureView::new_gd());
        if !rid.is_valid() {
            return Err("Godot could not create the output texture".into());
        }
        let godot = self
            .gpu
            .texture(&mut self.rd, rid)
            .map_err(|error| error.to_string())?;
        let texture = self.gpu.twin(&godot);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.display.set_texture_rd_rid(rid);
        if let Some(previous) = self.output.replace(Output {
            rid,
            godot,
            texture,
            view,
        }) {
            self.rd.free_rid(previous.rid);
        }
        Ok(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(output) = self.output.take() {
            self.display.set_texture_rd_rid(Rid::Invalid);
            self.rd.free_rid(output.rid);
        }
    }
}
