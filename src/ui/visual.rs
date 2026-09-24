//! The visual half of a search. The text matches show at once, and the files
//! that look like the query merge into them a moment later. `l` searches by the
//! look of the selection instead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use gpui::prelude::*;
use gpui::{Context, ScrollStrategy, SharedString, Task, Window};

use crate::clip::text::TextEncoder;
use crate::db::{Db, vectors};
use crate::paths;
use crate::search::folder::Entry;
use crate::search::query::{like_label, visual_only, visual_text};
use crate::search::visual::{self, DELTA, FLOOR, LIKE_COUNT, Scored, Vectors};
use crate::search::{Hit, Request};
use crate::ui::{FileBuddy, Page, count};

/// How long typing must pause before the text encoder runs. About the time it
/// takes, so a fast typist does not queue a run per key.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Query vectors kept, so a query typed again or a result refresh does not run
/// the text encoder.
const CACHE_SIZE: usize = 64;

#[derive(Default)]
pub(super) struct Visual {
    /// None until the background load at start is done, and after it failed.
    encoder: Option<Arc<Mutex<Encoder>>>,
    /// Shared with the search that runs in the background. Copied only when
    /// new rows arrive while a search holds it.
    vectors: Arc<Vectors>,
    /// The search that waits or runs. A new one drops it, which cancels it.
    pub(super) task: Option<Task<()>>,
    /// Searches by look, the latest last. Escape leaves one at a time. While
    /// one is here, the search box shows its label.
    likes: Vec<Like>,
}

/// A search by the look of files, and the results it replaced.
struct Like {
    label: String,
    hashes: Vec<String>,
    before: Before,
}

/// What escape brings back.
struct Before {
    text: String,
    results: Vec<Entry>,
    selected: usize,
    marked: HashSet<String>,
    listing: bool,
    status: SharedString,
    /// The search for these results had not finished, so they are not all
    /// there.
    pending: bool,
}

struct Encoder {
    model: TextEncoder,
    cache: HashMap<String, Vec<f32>>,
    order: VecDeque<String>,
}

impl Encoder {
    fn embed(&mut self, words: &str) -> Result<Vec<f32>> {
        if let Some(vector) = self.cache.get(words) {
            return Ok(vector.clone());
        }
        let vector = self.model.embed(words)?;
        if self.order.len() == CACHE_SIZE
            && let Some(oldest) = self.order.pop_front()
        {
            self.cache.remove(&oldest);
        }
        self.order.push_back(words.to_string());
        self.cache.insert(words.to_string(), vector.clone());
        Ok(vector)
    }
}

impl FileBuddy {
    /// Load the text encoder and every vector, away from the window. Without
    /// the model, search is text only.
    pub(super) fn load_visual(&mut self, cx: &mut Context<Self>) {
        let db_path = self.db_path.clone();
        let load = cx.background_spawn(async move {
            let model = TextEncoder::load(&paths::clip_model()?)?;
            let mut vectors = Vectors::default();
            vectors.refresh(Db::open(&db_path)?.conn())?;
            Ok::<_, anyhow::Error>((model, vectors))
        });
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            this.update(cx, |this, cx| match loaded {
                Ok((model, vectors)) => {
                    this.visual.encoder = Some(Arc::new(Mutex::new(Encoder {
                        model,
                        cache: HashMap::new(),
                        order: VecDeque::new(),
                    })));
                    this.visual.vectors = Arc::new(vectors);
                    this.rerun_search(cx);
                }
                Err(e) => log::warn!("visual search is off: {e:#}"),
            })
            .ok();
        })
        .detach();
    }

    /// Look for files that show what `request` says, then merge them into
    /// `text`, the text matches already shown.
    pub(super) fn search_visually(
        &mut self,
        request: Request,
        text: Vec<Hit>,
        cx: &mut Context<Self>,
    ) {
        self.visual.task = None;
        let only = visual_only(&request.text).is_some();
        let Some(words) = visual_text(&request.text) else {
            return;
        };
        if self.config.weights.visual <= 0.0 && !only {
            return;
        }
        let Some(encoder) = self.visual.encoder.clone() else {
            if only {
                self.status = "visual search is not ready".into();
            }
            return;
        };
        if let Err(e) = self.refresh_vectors() {
            log::error!("reading the vectors failed: {e:#}");
        }
        let vectors = self.visual.vectors.clone();
        self.visual.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let scored = cx
                .background_spawn(async move {
                    let query = encoder
                        .lock()
                        .map_err(|_| anyhow!("the text encoder lock is poisoned"))?
                        .embed(&words)?;
                    Ok::<_, anyhow::Error>(vectors.search(&query, FLOOR, DELTA))
                })
                .await;
            this.update(cx, |this, cx| this.merge_visual(request, text, scored, cx))
                .ok();
        }));
    }

    /// The rows the `clip` stage wrote since the last search.
    fn refresh_vectors(&mut self) -> Result<()> {
        let rows = vectors::since(self.db.conn(), self.visual.vectors.last_id())?;
        if !rows.is_empty() {
            Arc::make_mut(&mut self.visual.vectors).apply(rows);
        }
        Ok(())
    }

    /// `a`. The results become the files that look like the marked files, or
    /// like the selection when nothing is marked.
    pub(super) fn more_like_this(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.page(), Page::Browse | Page::Folders) {
            return;
        }
        let hashes = self.action_targets();
        if hashes.is_empty() {
            return;
        }
        if self.visual.encoder.is_none() {
            self.flash("visual search is not ready", true, cx);
            return;
        }
        if let Err(e) = self.refresh_vectors() {
            log::error!("reading the vectors failed: {e:#}");
        }
        if !hashes.iter().any(|hash| self.visual.vectors.contains(hash)) {
            self.flash("no frames are embedded for this yet", true, cx);
            return;
        }
        let names: Vec<String> = hashes
            .iter()
            .map(|hash| {
                self.results
                    .iter()
                    .filter_map(Entry::hit)
                    .find(|hit| hit.file.hash == *hash)
                    .map(|hit| hit.file.filename.clone())
                    .unwrap_or_default()
            })
            .collect();
        let label = like_label(&names);
        let before = Before {
            text: self.input.read(cx).value().to_string(),
            results: self.results.clone(),
            selected: self.selected,
            marked: self.marked.clone(),
            listing: self.listing,
            status: self.status.clone(),
            pending: self
                .visual
                .task
                .as_ref()
                .is_some_and(|task| !task.is_ready()),
        };
        self.input
            .update(cx, |input, cx| input.set_value(label.clone(), window, cx));
        self.visual.likes.push(Like {
            label,
            hashes,
            before,
        });
        self.run_search(cx);
    }

    /// Typing, or another folder. The results from before are no longer the
    /// way back.
    pub(super) fn end_likes(&mut self) {
        self.visual.likes.clear();
    }

    /// Run the latest search by look again, if there is one. `fresh` starts at
    /// the top of the results, as `show_results` does.
    pub(super) fn search_like(&mut self, fresh: bool, cx: &mut Context<Self>) -> bool {
        let Some(like) = self.visual.likes.last() else {
            return false;
        };
        let text = like.label.clone();
        let hashes = like.hashes.clone();
        self.visual.task = None;
        self.listing = false;
        if let Err(e) = self.refresh_vectors() {
            log::error!("reading the vectors failed: {e:#}");
        }
        let request = Request::new(text)
            .kinds(self.kinds.clone())
            .within(self.scope())
            .limit(LIKE_COUNT);
        let vectors = self.visual.vectors.clone();
        self.visual.task = Some(cx.spawn(async move |this, cx| {
            let scored = cx
                .background_spawn(async move { vectors.like(&hashes) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                match visual::hits(this.db.conn(), &request, &scored) {
                    Ok(hits) => {
                        this.status = count(hits.len());
                        this.read_row_tags();
                        let results = hits.into_iter().map(Entry::File).collect();
                        if fresh {
                            this.show_results(results, cx);
                        } else {
                            this.update_results(results, cx);
                        }
                    }
                    Err(e) => {
                        log::error!("search by look failed: {e:#}");
                        this.status = format!("search by look failed: {e:#}").into();
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
        true
    }

    /// Escape, after everything else it undoes. The results from before the
    /// last `l` come back as they were, and a search for them that had not
    /// finished runs again.
    pub(super) fn leave_like(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(like) = self.visual.likes.pop() else {
            return;
        };
        let before = like.before;
        self.visual.task = None;
        self.input.update(cx, |input, cx| {
            input.set_value(before.text.clone(), window, cx)
        });
        self.listing = before.listing;
        self.status = before.status;
        self.read_row_tags();
        let text_hits: Vec<Hit> = before
            .results
            .iter()
            .filter_map(Entry::hit)
            .cloned()
            .collect();
        self.show_results(before.results, cx);
        self.marked = before.marked;
        self.select(before.selected, cx);
        if !before.pending {
            return;
        }
        if self.visual.likes.is_empty() {
            if !before.listing {
                let request = Request::new(before.text)
                    .kinds(self.kinds.clone())
                    .within(self.scope());
                self.search_visually(request, text_hits, cx);
            }
        } else {
            self.search_like(false, cx);
        }
    }

    fn merge_visual(
        &mut self,
        request: Request,
        text: Vec<Hit>,
        scored: Result<Vec<Scored>>,
        cx: &mut Context<Self>,
    ) {
        let merged = scored
            .and_then(|scored| visual::hits(self.db.conn(), &request, &scored))
            .map(|found| visual::fuse(text, found, self.config.weights.visual, request.limit));
        match merged {
            Ok(hits) => {
                self.status = count(hits.len());
                let results = hits.into_iter().map(Entry::File).collect();
                // A selection the user has not moved stays on the first result.
                // Following the old first file down the grid moves the view
                // for no reason the user can see.
                let untouched = self.selected == 0;
                self.update_results(results, cx);
                if untouched && self.selected != 0 {
                    self.selected = 0;
                    self.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    self.schedule_preview(cx);
                }
            }
            Err(e) => {
                log::error!("visual search failed: {e:#}");
                self.status = format!("visual search failed: {e:#}").into();
                cx.notify();
            }
        }
    }
}
