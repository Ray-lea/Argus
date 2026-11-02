//! Bound content measurement to the visible rows and cache shaped widths.
use super::*;
use std::ops::Range;

pub(super) type CellValues = HashMap<(usize, &'static str), String>;

#[derive(Default)]
pub(super) struct AutoWidths {
    pub widths: HashMap<&'static str, f32>,
    measured: HashMap<(u8, String), (f32, u64)>,
    epoch: u64,
}

pub(super) struct Contents<'a> {
    pub rows: &'a [Row],
    pub range: Range<usize>,
    pub totals: &'a HashMap<String, String>,
    pub values: &'a mut CellValues,
    pub bits: bool,
}

impl AutoWidths {
    pub fn reset(&mut self, fonts: bool) {
        self.widths.clear();
        if fonts {
            self.measured.clear();
        }
    }

    fn measure(
        &mut self,
        text: &str,
        role: u8,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        if text.is_empty() {
            return 0.;
        }
        if text.len() > 1024 {
            return 2400.;
        }
        let text = text.replace(['\r', '\n', '\t'], " ");
        let key = (role, text.clone());
        if let Some((width, epoch)) = self.measured.get_mut(&key) {
            *epoch = self.epoch;
            return *width;
        }
        let size = theme::font(match role {
            0 => 12.,
            1 => 15.,
            _ => 13.,
        });
        let mut run = theme::text_run(&text, theme::LABEL_WEIGHT, cx);
        if role == 1 {
            run.font.family = theme::display_font(cx);
        }
        let width = f32::from(system.layout_line(&text, size, &[run], None).width).ceil();
        if self.measured.len() >= 4096 {
            let epoch = self.epoch;
            self.measured
                .retain(|_, (_, seen)| epoch.saturating_sub(*seen) <= 2);
            if self.measured.len() >= 4096 {
                self.measured.clear();
            }
        }
        self.measured.insert(key, (width, self.epoch));
        width
    }

    pub fn apply(&mut self, fields: &mut [Field], contents: Contents<'_>, cx: &App) {
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        for field in fields {
            let header = self.measure(&field.label, 0, &system, cx) + CELL_PADDING * 2. + 20.;
            let total = self.measure(
                contents
                    .totals
                    .get(field.key)
                    .map(String::as_str)
                    .unwrap_or(""),
                1,
                &system,
                cx,
            ) + CELL_PADDING * 2.
                + 6.;
            let mut required = header.max(total);
            for index in contents.range.clone() {
                let Some(row) = contents.rows.get(index) else {
                    continue;
                };
                let text = contents
                    .values
                    .entry((index, field.key))
                    .or_insert_with(|| display_cell(row, field, contents.bits));
                let mut width = self.measure(text, 2, &system, cx) + CELL_PADDING * 2.;
                if field.key == "name" {
                    width += 48. + row.depth as f32 * 16.;
                    if !row.detail.is_empty() {
                        width += self.measure(&row.detail, 0, &system, cx) + 10.;
                    }
                }
                required = required.max(width);
            }
            let (minimum, limit): (f32, f32) = match field.key {
                "name" => (180., 400.),
                "path" => (120., 600.),
                "description" => (100., 480.),
                "company" | "gpuEngine" => (100., 360.),
                _ => (64., 400.),
            };
            required = required.clamp(minimum, limit.max(header).max(total));
            let width = self.widths.entry(field.key).or_insert(required);
            *width = width.max(required);
            field.track = if field.key == "name" {
                Track::Flex(*width, 1.)
            } else {
                Track::Fixed(*width)
            };
        }
    }
}
