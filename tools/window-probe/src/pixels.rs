//! Compare owned window captures after decoding, independently of PNG compression.
use anyhow::{Context, ensure};
use std::{fs::File, io::BufReader, path::Path};

pub(crate) fn read(path: impl AsRef<Path>) -> anyhow::Result<(u32, u32, Vec<u8>)> {
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

/// Luma range between the 0.5th and 99.5th percentiles. Ignore isolated
/// highlights so one bright pixel cannot hide a blank frame in the crop.
fn contrast(pixels: &[u8]) -> u8 {
    let mut histogram = [0usize; 256];
    for &[r, g, b] in pixels.as_chunks::<3>().0 {
        let y = 54 * usize::from(r) + 183 * usize::from(g) + 19 * usize::from(b);
        histogram[y >> 8] += 1;
    }
    let count = pixels.len() / 3;
    let tail = count / 200;
    let level = |rank| {
        let mut sum = 0;
        histogram
            .iter()
            .position(|n| {
                sum += n;
                sum > rank
            })
            .expect("nonempty image") as u8
    };
    level(count - 1 - tail) - level(tail)
}

pub fn frame_contrast(directory: &str) -> anyhow::Result<()> {
    let directory = Path::new(directory);
    let index: serde_json::Value =
        serde_json::from_reader(File::open(directory.join("index.json"))?)?;
    let frames = index["frames"]
        .as_array()
        .context("missing recorded frames")?;
    let mut levels = Vec::with_capacity(frames.len());
    for i in 0..frames.len() {
        let (_, _, pixels) = read(directory.join(format!("{i:04}.png")))?;
        levels.push(contrast(&pixels));
    }
    println!("{}", serde_json::to_string(&levels)?);
    Ok(())
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

/// A rectangle of an image in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    fn parse(values: &[String]) -> anyhow::Result<Self> {
        let [x, y, width, height] = values else {
            anyhow::bail!("region needs X Y WIDTH HEIGHT");
        };
        Ok(Self {
            x: x.parse()?,
            y: y.parse()?,
            width: width.parse()?,
            height: height.parse()?,
        })
    }
}

/// `region` of an RGB image (clamped to it), each pixel repeated `scale`
/// times across and down so small details read at a glance.
fn cut(width: u32, height: u32, pixels: &[u8], region: Region, scale: u32) -> (u32, u32, Vec<u8>) {
    let x = region.x.min(width);
    let y = region.y.min(height);
    let (w, h) = (region.width.min(width - x), region.height.min(height - y));
    let scale = scale.max(1);
    let (out_width, out_height) = (w * scale, h * scale);
    let mut out = Vec::with_capacity((out_width * out_height * 3) as usize);
    for row in 0..out_height {
        let source = (y + row / scale) * width;
        for col in 0..out_width {
            let i = ((source + x + col / scale) * 3) as usize;
            out.extend_from_slice(&pixels[i..i + 3]);
        }
    }
    (out_width, out_height, out)
}

fn write_rgb(path: &str, width: u32, height: u32, pixels: &[u8]) -> anyhow::Result<()> {
    let mut encoder =
        png::Encoder::new(std::io::BufWriter::new(File::create(path)?), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}

/// `crop IN OUT X Y WIDTH HEIGHT [SCALE]`: one region of a capture, enlarged.
pub fn crop(args: &[String]) -> anyhow::Result<()> {
    ensure!(args.len() >= 6, "crop IN OUT X Y WIDTH HEIGHT [SCALE]");
    let (width, height, pixels) = read(&args[0])?;
    let scale = args.get(6).map(|s| s.parse()).transpose()?.unwrap_or(1);
    let (w, h, out) = cut(width, height, &pixels, Region::parse(&args[2..6])?, scale);
    ensure!(w > 0 && h > 0, "the region is outside the image");
    write_rgb(&args[1], w, h, &out)?;
    println!("{}", serde_json::json!({"width":w,"height":h}));
    Ok(())
}

/// Places tiles left to right in rows of `columns`, `GAP` apart on a dark ground.
fn tile(tiles: &[(u32, u32, Vec<u8>)], columns: u32) -> (u32, u32, Vec<u8>) {
    const GAP: u32 = 4;
    let columns = columns.max(1).min(tiles.len().max(1) as u32);
    let rows = (tiles.len() as u32).div_ceil(columns);
    let cell_width = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let cell_height = tiles.iter().map(|t| t.1).max().unwrap_or(0);
    let width = columns * cell_width + (columns + 1) * GAP;
    let height = rows * cell_height + (rows + 1) * GAP;
    let mut out = vec![24u8; (width * height * 3) as usize];
    for (index, (w, h, pixels)) in tiles.iter().enumerate() {
        let left = GAP + (index as u32 % columns) * (cell_width + GAP);
        let top = GAP + (index as u32 / columns) * (cell_height + GAP);
        for row in 0..*h {
            let from = (row * w * 3) as usize;
            let to = (((top + row) * width + left) * 3) as usize;
            out[to..to + (w * 3) as usize].copy_from_slice(&pixels[from..from + (w * 3) as usize]);
        }
    }
    (width, height, out)
}

/// `sheet OUT X Y WIDTH HEIGHT COLUMNS SCALE IN...`: the same region of each
/// capture side by side, in order, to judge a sequence of frames at once.
pub fn sheet(args: &[String]) -> anyhow::Result<()> {
    ensure!(
        args.len() >= 8,
        "sheet OUT X Y WIDTH HEIGHT COLUMNS SCALE IN..."
    );
    let region = Region::parse(&args[1..5])?;
    let columns = args[5].parse()?;
    let scale = args[6].parse()?;
    let mut tiles = Vec::with_capacity(args.len() - 7);
    for path in &args[7..] {
        let (width, height, pixels) = read(path).with_context(|| path.clone())?;
        tiles.push(cut(width, height, &pixels, region, scale));
    }
    let (width, height, out) = tile(&tiles, columns);
    write_rgb(&args[0], width, height, &out)?;
    println!(
        "{}",
        serde_json::json!({"width":width,"height":height,"tiles":tiles.len()})
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{Region, cut, stroke_x, tile};

    #[test]
    fn a_cut_is_clamped_to_the_image_and_scaled_by_repeating_pixels() {
        // 2 × 2: red, green / blue, white.
        let image = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let region = Region {
            x: 1,
            y: 0,
            width: 5,
            height: 5,
        };
        let (w, h, pixels) = cut(2, 2, &image, region, 2);
        assert_eq!((w, h), (2, 4));
        assert_eq!(&pixels[..6], &[0, 255, 0, 0, 255, 0]);
        assert_eq!(&pixels[18..], &[255, 255, 255, 255, 255, 255]);
        let outside = Region {
            x: 9,
            y: 9,
            width: 3,
            height: 3,
        };
        assert_eq!(cut(2, 2, &image, outside, 1).0, 0);
    }

    #[test]
    fn tiles_fill_rows_in_order_with_gaps() {
        let one = (1, 1, vec![9, 9, 9]);
        let (width, height, pixels) = tile(&[one.clone(), one.clone(), one], 2);
        // Two columns and two rows of 1 px cells, 4 px apart and from the edges.
        assert_eq!((width, height), (2 + 3 * 4, 2 + 3 * 4));
        let at = |x: u32, y: u32| &pixels[((y * width + x) * 3) as usize..][..3];
        assert_eq!(at(4, 4), [9, 9, 9]);
        assert_eq!(at(9, 4), [9, 9, 9]);
        assert_eq!(at(4, 9), [9, 9, 9]);
        assert_eq!(at(9, 9), [24, 24, 24]);
    }

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
