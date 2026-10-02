use std::{
    fs::File,
    io::{BufWriter, Write},
    rc::Rc,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or("usage: snapshot OUTPUT.ppm WIDTH HEIGHT DENSITY FRAMES [--app-options]")?;
    let width: u32 = args.get(2).ok_or("missing width")?.parse()?;
    let height: u32 = args.get(3).ok_or("missing height")?.parse()?;
    let density: f32 = args.get(4).ok_or("missing density")?.parse()?;
    let frames: u64 = args.get(5).ok_or("missing frames")?.parse()?;
    if frames == 0 {
        return Err("frames must be nonzero".into());
    }
    cranpose::set_platform_launch_args(Rc::new(cranpose::launch_args_from_command_line(
        args.iter().skip(6).cloned(),
        true,
    )));
    let mut app = cranpose_watchos_runner::create_application(width, height, density)?;
    app.set_active(true);
    for frame in 0..frames {
        app.tick(frame.saturating_mul(16_666_667));
    }
    let mut image = BufWriter::new(File::create(path)?);
    write!(image, "P6\n{width} {height}\n255\n")?;
    for pixel in app.pixels().chunks_exact(4) {
        image.write_all(&pixel[..3])?;
    }
    image.flush()?;
    println!("Rendered {frames} frames at {width}x{height} into {path}");
    Ok(())
}
