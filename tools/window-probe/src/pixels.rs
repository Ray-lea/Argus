//! Compare owned window captures after decoding, independently of PNG compression.
use anyhow::{Context, ensure};
use std::{fs::File, io::BufReader, path::Path};

fn read(path: impl AsRef<Path>) -> anyhow::Result<(u32, u32, Vec<u8>)> {
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

/// Find both slopes of the synthetic peak in each scanline. Weight the pixels
/// of each stroke by coverage, then give the two slopes equal weight so changes
/// in brightness at a grid line do not look like horizontal movement.
fn stroke_x(width: u32, pixels: &[u8]) -> Option<f64> {
    let rows = pixels.as_chunks::<3>().0.chunks_exact(width as usize);
    let height = rows.len();
    let mut positions = 0.;
    let mut count = 0;
    for row in rows {
        if let Some(center) = peak_center(row) {
            positions += center;
            count += 1;
        }
    }
    (count != 0 && 4 * count >= 3 * height).then(|| positions / count as f64)
}

fn peak_center(row: &[[u8; 3]]) -> Option<f64> {
    let mut slopes = [0.; 2];
    let mut count = 0;
    let (mut weight, mut moment) = (0u64, 0u64);
    for (x, &[red, green, blue]) in row.iter().chain(std::iter::once(&[0, 0, 0])).enumerate() {
        let contrast = blue.saturating_sub(red);
        if contrast > 62 && green.saturating_sub(red) > 45 {
            let coverage = u64::from(contrast - 45);
            moment += coverage * x as u64;
            weight += coverage;
        } else if weight > 0 {
            *slopes.get_mut(count)? = moment as f64 / weight as f64;
            count += 1;
            (weight, moment) = (0, 0);
        }
    }
    (count == 2 && slopes[1] - slopes[0] > 2.).then(|| (slopes[0] + slopes[1]) * 0.5)
}

pub fn track_chart(directory: &str) -> anyhow::Result<()> {
    let directory = Path::new(directory);
    let index: serde_json::Value =
        serde_json::from_reader(File::open(directory.join("index.json"))?)?;
    let frames = index["frames"]
        .as_array()
        .context("missing recorded frames")?;
    let mut first = None;
    let mut leftmost = f64::INFINITY;
    let mut last = 0.;
    let mut max_reverse = 0.;
    let mut worst_frame = None;
    let mut tracked = 0;
    let mut last_time = None;
    for (i, frame) in frames.iter().enumerate() {
        let time = frame["time100ns"]
            .as_i64()
            .context("missing frame timestamp")?;
        if last_time.is_some_and(|t| time <= t) {
            continue;
        }
        last_time = Some(time);
        let (width, _, pixels) = read(directory.join(format!("{i:04}.png")))?;
        let Some(x) = stroke_x(width, &pixels) else {
            continue;
        };
        first.get_or_insert(x);
        leftmost = leftmost.min(x);
        if x - leftmost > max_reverse {
            max_reverse = x - leftmost;
            worst_frame = Some(i);
        }
        last = x;
        tracked += 1;
    }
    let first = first.context("CPU stroke missing from the recording")?;
    println!(
        "{}",
        serde_json::json!({"frames":frames.len(),"tracked":tracked,"travelPx":first-last,"maxReversePx":max_reverse,"worstFrame":worst_frame})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::stroke_x;

    #[test]
    fn cyan_stroke_position_keeps_subpixels_and_ignores_gray_pixels() {
        let row = [
            [210, 210, 210],
            [30, 199, 230],
            [30, 199, 230],
            [60, 66, 72],
            [0, 0, 0],
            [20, 130, 150],
        ];
        assert_eq!(stroke_x(6, row.as_flattened()), Some(3.25));
        assert_eq!(stroke_x(1, &[240, 240, 240]), None);
    }
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
    for (left, right) in before
        .as_chunks::<3>()
        .0
        .iter()
        .zip(after.as_chunks::<3>().0)
    {
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
