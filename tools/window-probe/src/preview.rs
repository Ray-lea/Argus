//! Publish the native SDR scene with sublevel Mica detail from a float capture.
use anyhow::{Result, ensure};

// One threshold per pixel in an 8x8 tile, shared by R, G and B.
const BAYER: [[u8; 8]; 8] = [
    [0, 48, 12, 60, 3, 51, 15, 63],
    [32, 16, 44, 28, 35, 19, 47, 31],
    [8, 56, 4, 52, 11, 59, 7, 55],
    [40, 24, 36, 20, 43, 27, 39, 23],
    [2, 50, 14, 62, 1, 49, 13, 61],
    [34, 18, 46, 30, 33, 17, 45, 29],
    [10, 58, 6, 54, 9, 57, 5, 53],
    [42, 26, 38, 22, 41, 25, 37, 21],
];

fn srgb_level(bytes: [u8; 2]) -> f64 {
    let linear = f64::from(half::f16::from_le_bytes(bytes).to_f32()).max(0.);
    255. * if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1. / 2.4) - 0.055
    }
}

// SDR and scRGB blend the same translucent material in different color spaces.
// Fit the local range of each material to its measured SDR tone per channel.
#[derive(Clone, Copy, Default)]
struct Fit {
    n: usize,
    x: f64,
    y: f64,
    xx: f64,
    xy: f64,
}
impl Fit {
    fn sample(&mut self, x: f64, y: u8) {
        self.n += 1;
        self.x += x;
        self.y += f64::from(y);
        self.xx += x * x;
        self.xy += x * f64::from(y);
    }
    fn line(self) -> (f64, f64) {
        let n = self.n.max(1) as f64;
        let variance = self.xx / n - (self.x / n).powi(2);
        let slope = if variance > 0.001 {
            (self.xy / n - self.x * self.y / n.powi(2)) / variance
        } else {
            1.
        };
        (slope, (self.y - slope * self.x) / n)
    }
}

pub fn background(
    w: usize,
    h: usize,
    fp16: &[u8],
    inactive: &str,
    active: &str,
) -> Result<Vec<u8>> {
    let (iw, ih, guide) = crate::pixels::read(inactive)?;
    let (aw, ah, mut scene) = crate::pixels::read(active)?;
    ensure!(
        (iw, ih) == (aw, ah) && (aw as usize, ah as usize) == (w, h),
        "preview frames differ in size"
    );
    restore(w, &guide, &mut scene, fp16);
    Ok(scene)
}

fn restore(width: usize, guide: &[u8], scene: &mut [u8], fp16: &[u8]) {
    let mut fits = [[Fit::default(); 3]; 256];
    let count = scene.len() / 3;
    // The title bar and table have large, neutral, flat fills when inactive.
    // An unchanged pixel is solid UI. Keep its original SDR bytes.
    let mut samples = Vec::new();
    for (i, ((&base, &native), pixel)) in guide
        .as_chunks::<3>()
        .0
        .iter()
        .zip(scene.as_chunks::<3>().0)
        .zip(fp16.as_chunks::<8>().0)
        .enumerate()
    {
        if base != native && base[0] == base[1] && base[1] == base[2] {
            let levels =
                std::array::from_fn::<_, 3, _>(|k| srgb_level([pixel[k * 2], pixel[k * 2 + 1]]));
            let shade = usize::from(base[0]);
            for k in 0..3 {
                fits[shade][k].sample(levels[k], native[k]);
            }
            samples.push((i, shade, levels));
        }
    }
    let lines = fits.map(|channels| {
        // Exclude small neutral glyphs, rules and controls from the material mask.
        (channels[0].n > count / 32).then(|| channels.map(Fit::line))
    });
    for (i, shade, levels) in samples {
        if let Some(line) = lines[shade] {
            let threshold = (f64::from(BAYER[i / width % 8][i % width % 8]) + 0.5) / 64.;
            for k in 0..3 {
                let byte = &mut scene[3 * i + k];
                let native = f64::from(*byte);
                let (slope, offset) = line[k];
                let level = (slope * levels[k] + offset).clamp(native - 0.5, native + 0.5);
                *byte = (level + threshold) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::restore;

    #[test]
    fn sublevel_gradients_survive_sdr_export_without_changing_ink() {
        const WIDTH: usize = 264;
        for (base, start) in [(32, 37.), (249, 237.)] {
            let mut guide = Vec::new();
            let mut scene = Vec::new();
            let mut fp16 = Vec::new();
            for _ in 0..8 {
                for x in 0..WIDTH {
                    let level = start + (x / 8) as f64 * 0.25 + 0.125;
                    let ink = x >= 256;
                    guide.extend(if ink { [65, 80, 190] } else { [base; 3] });
                    scene.extend(if ink {
                        [67, 82, 192]
                    } else {
                        [level.round() as u8; 3]
                    });
                    let linear = ((level / 255. + 0.055) / 1.055).powf(2.4);
                    fp16.extend(half::f16::from_f64(linear).to_le_bytes().repeat(4));
                }
            }
            let native = scene.clone();
            restore(WIDTH, &guide, &mut scene, &fp16);
            for tile in 0..32 {
                let mut sum = 0.;
                for y in 0..8 {
                    for x in tile * 8..tile * 8 + 8 {
                        let i = (y * WIDTH + x) * 3;
                        sum += f64::from(scene[i]);
                        assert!(scene[i].abs_diff(native[i]) <= 1);
                        assert_eq!(scene[i..i + 2], scene[i + 1..i + 3]);
                    }
                    let ink = (y * WIDTH + 256) * 3;
                    assert_eq!(scene[ink..ink + 24], native[ink..ink + 24]);
                }
                assert!((sum / 64. - (start + tile as f64 * 0.25 + 0.125)).abs() < 0.1);
            }
        }
    }
}
