//! Compare owned window captures after decoding, independently of PNG compression.
use anyhow::{Context, ensure};
use std::{fs::File, io::BufReader};

fn read(path: &str) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(BufReader::new(File::open(path)?)).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().context("PNG size overflow")?];
    let info = reader.next_frame(&mut pixels)?;
    ensure!(
        info.color_type == png::ColorType::Rgb && info.bit_depth == png::BitDepth::Eight,
        "expected an RGB window capture"
    );
    pixels.truncate(info.buffer_size());
    Ok((info.width, info.height, pixels))
}

pub fn compare(left: &str, right: &str) -> anyhow::Result<()> {
    let (width, height, before) = read(left)?;
    let (other_width, other_height, after) = read(right)?;
    ensure!(
        (width, height) == (other_width, other_height),
        "capture dimensions differ"
    );
    let mut changed = 0;
    let mut maximum = 0;
    for (left, right) in before.chunks_exact(3).zip(after.chunks_exact(3)) {
        let delta = left
            .iter()
            .zip(right)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        changed += usize::from(delta != 0);
        maximum = maximum.max(delta);
    }
    println!(
        "{}",
        serde_json::json!({"width":width,"height":height,"changedPixels":changed,"maxChannelDelta":maximum})
    );
    Ok(())
}
