//! Search by what the frames show. Every CLIP vector is held in memory and
//! compared with the query, and the result merges with the FTS list.

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::clip::{self, Frame};
use crate::db::files::{PRESENT, read_row};
use crate::db::vectors;
use crate::media::Kind;
use crate::search::{Hit, Request, Signal, filters};

/// A file whose best frame scores below this matches nothing. CLIP always has a
/// best match, even for a query that nothing fits.
pub const FLOOR: f32 = 0.25;
/// How far below the best file of a query a file may score and still match.
pub const DELTA: f32 = 0.03;
/// How many files a search by look shows, after the chips and the folder. Scores between two files run from
/// about 0.6 to 0.97 and spread differently for each file, so no cutoff by
/// score fits them all.
pub const LIKE_COUNT: usize = 50;
/// The rank constant of reciprocal rank fusion (Cormack 2009).
const RRF_K: f64 = 60.0;

/// The vectors of every file, by hash.
#[derive(Debug, Default, Clone)]
pub struct Vectors {
    /// The last row read, so a refresh reads only newer rows.
    last_id: i64,
    files: HashMap<String, Vec<Frame>>,
}

/// One file's score for a query: its best frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub hash: String,
    pub score: f32,
    pub time_ms: i64,
}

impl Vectors {
    /// Read the rows the `clip` stage wrote since the last call.
    pub fn refresh(&mut self, conn: &Connection) -> Result<usize> {
        let rows = vectors::since(conn, self.last_id)?;
        Ok(self.apply(rows))
    }

    /// The last row read. Newer rows are what a refresh reads.
    pub fn last_id(&self) -> i64 {
        self.last_id
    }

    /// Add rows from `vectors::since`. A file that was embedded again loses its
    /// old frames.
    pub fn apply(&mut self, rows: Vec<(i64, String, Frame)>) -> usize {
        let count = rows.len();
        let mut replaced = std::collections::HashSet::new();
        for (id, hash, frame) in rows {
            self.last_id = id;
            let frames = self.files.entry(hash.clone()).or_default();
            if replaced.insert(hash) {
                frames.clear();
            }
            frames.push(frame);
        }
        count
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.files.contains_key(hash)
    }

    /// The files that match, best first: each file by its best frame, cut at
    /// `max(floor, top - delta)`.
    pub fn search(&self, query: &[f32], floor: f32, delta: f32) -> Vec<Scored> {
        let mut scored = self.score(query, &[]);
        cut(&mut scored, floor, delta);
        scored
    }

    /// Every other file, best first, by how much it looks like `hashes`.
    /// `None` when no file of `hashes` has a vector yet.
    pub fn like(&self, hashes: &[String]) -> Option<Vec<Scored>> {
        let query = self.mean(hashes)?;
        Some(self.score(&query, hashes))
    }

    /// The mean of each file's frames, then the mean of the files, so a video
    /// of ten frames counts as much as a still of one. Normalized.
    fn mean(&self, hashes: &[String]) -> Option<Vec<f32>> {
        let mut sum: Vec<f32> = Vec::new();
        let mut files = 0;
        for frames in hashes.iter().filter_map(|hash| self.files.get(hash)) {
            let Some(first) = frames.first() else {
                continue;
            };
            sum.resize(first.vector.len(), 0.0);
            let share = 1.0 / frames.len() as f32;
            for frame in frames {
                for (total, x) in sum.iter_mut().zip(&frame.vector) {
                    *total += x * share;
                }
            }
            files += 1;
        }
        if files == 0 {
            return None;
        }
        clip::normalize(&mut sum);
        Some(sum)
    }

    /// Every file but `skip`, each by its best frame, best first.
    fn score(&self, query: &[f32], skip: &[String]) -> Vec<Scored> {
        let mut scored: Vec<Scored> = self
            .files
            .iter()
            .filter(|(hash, _)| !skip.contains(hash))
            .filter_map(|(hash, frames)| {
                frames
                    .iter()
                    .map(|frame| (clip::dot(query, &frame.vector), frame.time_ms))
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(score, time_ms)| Scored {
                        hash: hash.clone(),
                        score,
                        time_ms,
                    })
            })
            .collect();
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        scored
    }
}

/// Keep only what scores at least `max(floor, top - delta)`. `scored` is best
/// first.
fn cut(scored: &mut Vec<Scored>, floor: f32, delta: f32) {
    let Some(top) = scored.first().map(|s| s.score) else {
        return;
    };
    let cutoff = floor.max(top - delta);
    scored.retain(|s| s.score >= cutoff);
}

/// The files on disk that hold the matched hashes, in the order of `scored`,
/// under the chips and the folder of the request.
pub fn hits(conn: &Connection, request: &Request, scored: &[Scored]) -> Result<Vec<Hit>> {
    if scored.is_empty() {
        return Ok(Vec::new());
    }
    let hashes: Vec<&str> = scored.iter().map(|s| s.hash.as_str()).collect();
    let mut values = vec![Value::Text(serde_json::to_string(&hashes)?)];
    let filter = filters(request, &mut values, "");
    let sql = format!("{PRESENT} AND hash IN (SELECT value FROM json_each(?1)){filter}");

    let rank: HashMap<&str, usize> = hashes.iter().enumerate().map(|(i, h)| (*h, i)).collect();
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), read_row)?;
    let mut hits = Vec::new();
    for row in rows {
        let file = row?;
        let at = rank[file.hash.as_str()];
        // A still has one frame, so a time in it says nothing.
        let at_ms = (file.kind != Kind::Still).then_some(scored[at].time_ms);
        hits.push((
            at,
            Hit {
                file,
                score: -f64::from(scored[at].score),
                signal: Signal::Visual,
                matched: String::new(),
                at_ms,
            },
        ));
    }
    hits.sort_by_key(|(at, hit)| (*at, hit.file.path.clone()));
    Ok(hits
        .into_iter()
        .map(|(_, hit)| hit)
        .take(request.limit)
        .collect())
}

/// Merge the FTS list and the visual list by weighted reciprocal rank fusion.
/// The FTS list has weight 1. A file in both lists keeps its text match, which
/// says more than a look does.
pub fn fuse(text: Vec<Hit>, visual: Vec<Hit>, visual_weight: f64, limit: usize) -> Vec<Hit> {
    let mut merged: Vec<(f64, Hit)> = Vec::new();
    let mut at: HashMap<i64, usize> = HashMap::new();
    for (rank, hit) in text.into_iter().enumerate() {
        at.insert(hit.file.id, merged.len());
        merged.push((1.0 / (RRF_K + rank as f64 + 1.0), hit));
    }
    for (rank, hit) in visual.into_iter().enumerate() {
        let score = visual_weight / (RRF_K + rank as f64 + 1.0);
        match at.get(&hit.file.id) {
            Some(&i) => merged[i].0 += score,
            None => {
                at.insert(hit.file.id, merged.len());
                merged.push((score, hit));
            }
        }
    }
    // Stable, so a tie keeps text before visual.
    merged.sort_by(|a, b| b.0.total_cmp(&a.0));
    merged
        .into_iter()
        .take(limit)
        .map(|(score, mut hit)| {
            hit.score = -score;
            hit
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::files::{FileRow, State};
    use crate::media::Kind;

    fn scored(hash: &str, score: f32) -> Scored {
        Scored {
            hash: hash.into(),
            score,
            time_ms: 0,
        }
    }

    fn hit(id: i64, signal: Signal) -> Hit {
        Hit {
            file: FileRow {
                id,
                path: format!("C:/clips/{id}.mp4").into(),
                hash: format!("h{id}"),
                filename: format!("{id}.mp4"),
                kind: Kind::Video,
                size: 1,
                duration: Some(1.0),
                state: State::Ready,
                mtime: 0,
            },
            score: 0.0,
            signal,
            matched: String::new(),
            at_ms: None,
        }
    }

    fn ids(hits: &[Hit]) -> Vec<i64> {
        hits.iter().map(|h| h.file.id).collect()
    }

    #[test]
    fn the_cut_follows_the_best_score() {
        let mut list = vec![
            scored("a", 0.33),
            scored("b", 0.31),
            scored("c", 0.29),
            scored("d", 0.26),
        ];
        cut(&mut list, 0.25, 0.03);
        assert_eq!(list, vec![scored("a", 0.33), scored("b", 0.31)]);
    }

    #[test]
    fn a_query_that_nothing_fits_keeps_nothing() {
        let mut list = vec![scored("a", 0.22), scored("b", 0.21)];
        cut(&mut list, 0.25, 0.03);
        assert!(list.is_empty());
    }

    #[test]
    fn the_floor_holds_when_the_best_is_weak() {
        let mut list = vec![scored("a", 0.26), scored("b", 0.25), scored("c", 0.24)];
        cut(&mut list, 0.25, 0.03);
        assert_eq!(list, vec![scored("a", 0.26), scored("b", 0.25)]);
    }

    #[test]
    fn each_file_scores_by_its_best_frame() {
        let mut vectors = Vectors::default();
        vectors.files.insert(
            "a".into(),
            vec![frame(0, [0.0, 1.0]), frame(500, [1.0, 0.0])],
        );
        vectors.files.insert("b".into(), vec![frame(0, [0.6, 0.8])]);
        let found = vectors.search(&[1.0, 0.0], 0.0, 1.0);
        assert_eq!(
            found,
            vec![
                Scored {
                    hash: "a".into(),
                    score: 1.0,
                    time_ms: 500
                },
                Scored {
                    hash: "b".into(),
                    score: 0.6,
                    time_ms: 0
                },
            ]
        );
    }

    fn frame(time_ms: i64, vector: [f32; 2]) -> Frame {
        Frame {
            time_ms,
            vector: vector.to_vec(),
        }
    }

    #[test]
    fn each_file_counts_once_in_the_mean_of_several() {
        let mut vectors = Vectors::default();
        vectors.files.insert(
            "a".into(),
            vec![
                frame(0, [1.0, 0.0]),
                frame(500, [1.0, 0.0]),
                frame(900, [1.0, 0.0]),
            ],
        );
        vectors.files.insert("b".into(), vec![frame(0, [0.0, 1.0])]);
        let mean = vectors.mean(&["a".into(), "b".into()]).unwrap();
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((mean[0] - half).abs() < 1e-6 && (mean[1] - half).abs() < 1e-6);
    }

    #[test]
    fn a_file_with_no_vectors_gives_no_query() {
        let vectors = Vectors::default();
        assert!(vectors.mean(&["a".into()]).is_none());
    }

    #[test]
    fn the_query_files_are_left_out() {
        let mut vectors = Vectors::default();
        vectors.files.insert("a".into(), vec![frame(0, [1.0, 0.0])]);
        vectors.files.insert("b".into(), vec![frame(0, [0.6, 0.8])]);
        vectors.files.insert("c".into(), vec![frame(0, [0.8, 0.6])]);
        let found = vectors.like(&["a".into(), "b".into()]).unwrap();
        let hashes: Vec<&str> = found.iter().map(|s| s.hash.as_str()).collect();
        assert_eq!(hashes, vec!["c"]);
    }

    #[test]
    fn a_text_match_stays_above_the_visual_hits() {
        let text = vec![hit(1, Signal::Filename), hit(2, Signal::Tags)];
        let visual = vec![hit(3, Signal::Visual), hit(4, Signal::Visual)];
        assert_eq!(ids(&fuse(text, visual, 0.5, 10)), vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_file_in_both_lists_rises_and_keeps_its_text_badge() {
        let text = vec![hit(1, Signal::Filename), hit(2, Signal::Speech)];
        let visual = vec![hit(2, Signal::Visual)];
        let merged = fuse(text, visual, 0.5, 10);
        assert_eq!(ids(&merged), vec![2, 1]);
        assert_eq!(merged[0].signal, Signal::Speech);
    }

    #[test]
    fn the_visual_weight_decides_where_a_visual_hit_lands() {
        let text: Vec<Hit> = (1..=40).map(|id| hit(id, Signal::Ocr)).collect();
        let visual = vec![hit(100, Signal::Visual)];
        // At weight 0.5 the best visual hit counts as much as text rank 62, so
        // after 40 text matches it comes last. At weight 1 it ties text rank 1
        // and comes right after it.
        assert_eq!(ids(&fuse(text.clone(), visual.clone(), 0.5, 100))[40], 100);
        assert_eq!(ids(&fuse(text, visual, 1.0, 100))[1], 100);
    }

    #[test]
    fn the_merge_stops_at_the_limit() {
        let text = vec![hit(1, Signal::Filename), hit(2, Signal::Filename)];
        let visual = vec![hit(3, Signal::Visual)];
        assert_eq!(ids(&fuse(text, visual, 0.5, 2)), vec![1, 2]);
    }
}
