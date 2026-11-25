use perf_data::*;

use super::ImageId;

pub(crate) struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}
pub(crate) struct Pictures<T> {
    avatars: Vec<T>,
    gif: Vec<T>,
    spinner: Option<T>,
}
impl<T> Pictures<T> {
    pub fn new(scenario: Scenario, mut upload: impl FnMut(Bitmap) -> T) -> Self {
        let mut result = Self {
            avatars: Vec::new(),
            gif: Vec::new(),
            spinner: None,
        };
        match scenario {
            Scenario::Gauntlet => {
                result.avatars = (0..AVATAR_COUNT)
                    .map(|i| {
                        upload(Bitmap {
                            w: AVATAR_SIZE,
                            h: AVATAR_SIZE,
                            rgba: avatar_rgba(i),
                        })
                    })
                    .collect()
            }
            Scenario::Patchwork => {
                let mut options = gif::DecodeOptions::new();
                options.set_color_output(gif::ColorOutput::RGBA);
                let mut decoder = options.read_info(CHAT_GIF).expect("bundled chat GIF");
                while let Some(frame) = decoder.read_next_frame().expect("GIF frame") {
                    result.gif.push(upload(Bitmap {
                        w: frame.width.into(),
                        h: frame.height.into(),
                        rgba: frame.buffer.to_vec(),
                    }));
                }
                let size = (CHAT_SPINNER_SIZE * 2.) as u32;
                let svg = resvg::usvg::Tree::from_str(CHAT_SVG, &Default::default())
                    .expect("bundled chat spinner");
                let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("spinner bitmap");
                resvg::render(
                    &svg,
                    resvg::tiny_skia::Transform::from_scale(
                        size as f32 / svg.size().width(),
                        size as f32 / svg.size().height(),
                    ),
                    &mut pixmap.as_mut(),
                );
                let rgba = pixmap
                    .pixels()
                    .iter()
                    .flat_map(|p| {
                        let p = p.demultiply();
                        [p.red(), p.green(), p.blue(), p.alpha()]
                    })
                    .collect();
                result.spinner = Some(upload(Bitmap {
                    w: size,
                    h: size,
                    rgba,
                }));
            }
            Scenario::Wall => {}
        }
        result
    }

    pub fn get(&self, id: ImageId) -> &T {
        match id {
            ImageId::Avatar(i) => &self.avatars[i % self.avatars.len()],
            ImageId::Gif(k) => &self.gif[chat_gif_frame(k as u32, self.gif.len())],
            ImageId::Spinner => self.spinner.as_ref().expect("Patchwork spinner"),
        }
    }
}
