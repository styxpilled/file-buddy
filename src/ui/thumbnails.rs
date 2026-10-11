//! The decoded thumbnails, at most `CAPACITY` of them. Without a cache of its
//! own, GPUI keeps every image it decodes for the life of the window, on the CPU
//! and in the sprite atlas, and a scroll through Browse decodes all of them.
//! Tiles show a smaller file than the pane, made on the first show when it is
//! missing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;
use gpui::prelude::*;
use gpui::{
    App, Asset, AssetLogger, Context, Entity, ImageAssetLoader, ImageCache, ImageCacheError,
    ImageCacheItem, RenderImage, Resource, Window, hash,
};

use crate::ffmpeg::thumbnail;
use crate::paths;

/// Five screens of tiles in a maximized window. The full thumbnail the pane
/// shows costs about 0.5 MB on the CPU and 0.8 MB of GPU memory, a tile
/// thumbnail at most 0.4 MB on the CPU.
const CAPACITY: usize = 200;

pub(super) struct Thumbnails {
    items: HashMap<u64, Item>,
    /// Counts loads, so the least recently shown thumbnail goes first.
    clock: u64,
    /// Hashes whose tile thumbnail is being made.
    making: HashSet<String>,
    /// Hashes whose tile thumbnail could not be made. Their tiles show the full
    /// thumbnail.
    failed: HashSet<String>,
}

struct Item {
    image: ImageCacheItem,
    used: u64,
}

impl Thumbnails {
    pub(super) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            items: HashMap::new(),
            clock: 0,
            making: HashSet::new(),
            failed: HashSet::new(),
        })
    }

    /// The picture a tile shows, or `None` while its file is made from the full
    /// thumbnail. The entity notifies when it is done.
    pub(super) fn tile(
        &mut self,
        hash: &str,
        cache: &Path,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        if self.failed.contains(hash) {
            return Some(paths::thumbnail_path(cache, hash));
        }
        let tile = paths::tile_path(cache, hash);
        if tile.is_file() {
            return Some(tile);
        }
        if self.making.insert(hash.to_string()) {
            let hash = hash.to_string();
            let cache = cache.to_path_buf();
            cx.spawn(async move |this, cx| {
                let made = cx
                    .background_spawn({
                        let hash = hash.clone();
                        async move { thumbnail::generate_tile(&hash, &cache) }
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.making.remove(&hash);
                    if let Err(e) = made {
                        log::warn!("tile thumbnail of {hash}: {e:#}");
                        this.failed.insert(hash);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }

    /// Let go of the pictures of these bytes, so new ones are read again.
    pub(super) fn forget(&mut self, hash: &str, cache: &Path) {
        for path in [
            paths::thumbnail_path(cache, hash),
            paths::tile_path(cache, hash),
        ] {
            self.items.remove(&gpui::hash(&Resource::Path(path.into())));
        }
        self.failed.remove(hash);
    }

    fn evict(&mut self, window: &mut Window, cx: &mut App) {
        let Some(oldest) = self
            .items
            .iter()
            .min_by_key(|(_, item)| item.used)
            .map(|(key, _)| *key)
        else {
            return;
        };
        if let Some(mut item) = self.items.remove(&oldest)
            && let Some(Ok(image)) = item.image.get()
        {
            cx.drop_image(image, Some(window));
        }
    }
}

impl ImageCache for Thumbnails {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        self.clock += 1;
        let key = hash(resource);
        if let Some(item) = self.items.get_mut(&key) {
            item.used = self.clock;
            return item.image.get();
        }

        let load = AssetLogger::<ImageAssetLoader>::load(resource.clone(), cx);
        let task = cx.background_executor().spawn(load).shared();
        self.items.insert(
            key,
            Item {
                image: ImageCacheItem::Loading(task.clone()),
                used: self.clock,
            },
        );
        let view = window.current_view();
        window
            .spawn(cx, async move |cx| {
                _ = task.await;
                cx.on_next_frame(move |_, cx| cx.notify(view));
            })
            .detach();
        if self.items.len() > CAPACITY {
            self.evict(window, cx);
        }
        None
    }
}
