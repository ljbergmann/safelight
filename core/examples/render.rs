//! The core's render on this computer, for comparing with the iPad's:
//! `cargo run --release --example render -- <photo> <out.ppm> [cpu|gpu] [full|half]`
//! prints the timings and writes the pixels as a binary PPM.

use rawmakase_core::{Backend, DecodeSize, render_photo};
use std::io::Write;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() >= 3,
        "usage: render <photo> <out.ppm> [cpu|gpu] [full|half]"
    );
    let backend = match args.get(3).map(String::as_str) {
        Some("cpu") => Backend::Cpu,
        _ => Backend::Gpu,
    };
    let size = match args.get(4).map(String::as_str) {
        Some("half") => DecodeSize::Half,
        _ => DecodeSize::Full,
    };
    let r = render_photo(args[1].clone(), 2048, size, backend)?;
    println!(
        "{} | decoded {}x{} | rendered {}x{} | open {:.0} ms, resolve {:.0} ms, decode {:.0} ms, renderer {:.0} ms, render {:.0} ms | {}",
        r.camera,
        r.decoded_width,
        r.decoded_height,
        r.width,
        r.height,
        r.open_ms,
        r.resolve_ms,
        r.decode_ms,
        r.renderer_ms,
        r.render_ms,
        r.backend
    );
    let mut f = std::fs::File::create(&args[2])?;
    write!(f, "P6\n{} {}\n255\n", r.width, r.height)?;
    let rgb: Vec<u8> = r
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, _]| [r, g, b])
        .collect();
    f.write_all(&rgb)?;
    Ok(())
}
