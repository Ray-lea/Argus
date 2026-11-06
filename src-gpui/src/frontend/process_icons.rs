//! One icon owner for the displayed snapshot, shared by tables and details.
use gpui_kit::gpui::{App, Image, ImageFormat, Window};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

struct Entry {
    // Pin the core's PNG while a paused or hidden snapshot can still show it.
    png: Arc<[u8]>,
    image: Option<Arc<Image>>,
}

#[derive(Default)]
pub(super) struct ProcessIcons {
    entries: HashMap<u32, Entry>,
}

impl ProcessIcons {
    pub fn synchronize(
        &mut self,
        ids: HashSet<u32>,
        mut load: impl FnMut(u32) -> Option<Arc<[u8]>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.entries.retain(|id, entry| {
            if ids.contains(id) {
                return true;
            }
            if let Some(image) = entry.image.take() {
                // Decoded assets and atlas tiles have separate ownership in GPUI.
                // A pending load has no tiles; removing the asset cancels it.
                let rendered = image.clone().get_render_image(window, cx);
                image.remove_asset(cx);
                if let Some(rendered) = rendered {
                    cx.drop_image(rendered, Some(window));
                }
            }
            false
        });
        for id in ids {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.entries.entry(id)
                && let Some(png) = load(id)
            {
                entry.insert(Entry { png, image: None });
            }
        }
        let reserve = self.entries.len() + 64;
        if self.entries.capacity() > reserve * 2 {
            self.entries.shrink_to(reserve);
        }
    }

    pub fn get(&mut self, id: u32) -> Option<Arc<Image>> {
        let entry = self.entries.get_mut(&id)?;
        Some(
            entry
                .image
                .get_or_insert_with(|| {
                    Arc::new(Image::from_bytes(ImageFormat::Png, entry.png.to_vec()))
                })
                .clone(),
        )
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessIcons;
    use gpui_test::{AppContext, Context, Image, IntoElement, Render, TestAppContext, Window, img};
    use std::{collections::HashSet, sync::Arc};

    struct Picture(Arc<Image>);
    impl Render for Picture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            img(self.0.clone())
        }
    }

    #[test]
    fn replacing_the_snapshot_releases_decoded_assets_and_pins_only_displayed_pngs() {
        let mut cx = TestAppContext::single();
        let png: Arc<[u8]> = include_bytes!("../../../resources/icons/32x32.png")
            .as_slice()
            .into();
        let source = Arc::downgrade(&png);
        let mut icons = ProcessIcons::default();
        let window = cx.add_window(|_, _| {
            Picture(Arc::new(Image::from_bytes(
                gpui_test::ImageFormat::Png,
                png.to_vec(),
            )))
        });
        cx.update_window(window.into(), |_, window, cx| {
            icons.synchronize(HashSet::from([1]), |_| Some(png.clone()), window, cx)
        })
        .unwrap();
        drop(png);
        let image = icons.get(1).unwrap();
        assert!(Arc::ptr_eq(&image, &icons.get(1).unwrap()));
        window
            .update(&mut cx, |view, _, _| view.0 = image.clone())
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let decoded = cx
            .update_window(window.into(), |_, window, cx| {
                image.clone().get_render_image(window, cx).unwrap()
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, _| {
            assert!(window.has_image_atlas_entry(&decoded))
        })
        .unwrap();
        cx.update(|cx| assert!(image.is_asset_cached(cx)));
        assert!(source.upgrade().is_some());
        cx.update_window(window.into(), |_, window, cx| {
            icons.synchronize(HashSet::new(), |_| unreachable!(), window, cx)
        })
        .unwrap();
        cx.update(|cx| assert!(!image.is_asset_cached(cx)));
        cx.update_window(window.into(), |_, window, _| {
            assert!(!window.has_image_atlas_entry(&decoded))
        })
        .unwrap();
        assert!(source.upgrade().is_none());
        assert_eq!(icons.len(), 0);
    }
}
