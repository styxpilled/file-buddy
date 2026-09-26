//! One FTS5 query over four columns, weighted by the config. `query` turns what
//! the user typed into the expression this runs.

pub mod folder;
pub mod moment;
pub mod query;
pub mod visual;

use std::path::PathBuf;

use anyhow::Result;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use self::query::{build_match, visual_only};
use crate::config::Weights;
use crate::db::files::{self, FileRow, PRESENT, read_row};
use crate::db::{ocr, segments};
use crate::media::{self, Kind};
use crate::search::folder::Sort;

/// Which column gave a result its best score. The grid shows it as a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Tags,
    Filename,
    Speech,
    Ocr,
    /// What a frame shows. Not a column of `fts`.
    Visual,
}

impl Signal {
    /// In the column order of `fts`.
    const COLUMNS: [Signal; 4] = [Signal::Tags, Signal::Filename, Signal::Speech, Signal::Ocr];

    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Tags => "tag",
            Signal::Filename => "filename",
            Signal::Speech => "speech",
            Signal::Ocr => "screen text",
            Signal::Visual => "visual",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub file: FileRow,
    /// Lower is better, and it is always negative: the bm25 score, or minus the
    /// fused score once the visual hits are merged in.
    pub score: f64,
    pub signal: Signal,
    /// The text that matched, from the column the badge names.
    pub matched: String,
    /// Where in the clip it matched: the speech segment, or the frame that
    /// looks most like the query.
    pub at_ms: Option<i64>,
    /// The query matched a tag or the filename, whatever the badge says. Such a
    /// file stays above the others when the visual hits merge in.
    pub named: bool,
}

impl Hit {
    /// The second line of a tile: the time in the clip, then what matched.
    pub fn snippet(&self) -> String {
        match self.at_ms {
            Some(ms) if self.matched.is_empty() => media::clock(ms),
            Some(ms) => format!("{} {}", media::clock(ms), self.matched),
            None => self.matched.clone(),
        }
    }
}

/// What to search for. The chips decide `kinds`.
#[derive(Debug, Clone)]
pub struct Request {
    pub text: String,
    /// The kinds the chips allow. Empty means every kind.
    pub kinds: Vec<Kind>,
    /// Only files under this folder, at any depth.
    pub within: Option<PathBuf>,
    /// The most results of a query. Browse lists every file.
    pub limit: usize,
    /// The order of Browse. A query keeps its ranking.
    pub sort: Sort,
}

impl Request {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kinds: Vec::new(),
            within: None,
            limit: 500,
            sort: Sort::Recent,
        }
    }

    pub fn kinds(mut self, kinds: Vec<Kind>) -> Self {
        self.kinds = kinds;
        self
    }

    pub fn within(mut self, folder: Option<PathBuf>) -> Self {
        self.within = folder;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn sort(mut self, sort: Sort) -> Self {
        self.sort = sort;
        self
    }
}

/// The best text matches for what the user typed. A query that starts with `~`
/// has none, and `visual` gives its results.
pub fn search(conn: &Connection, request: &Request, weights: &Weights) -> Result<Vec<Hit>> {
    let text = match visual_only(&request.text) {
        Some("") => "",
        Some(_) => return Ok(Vec::new()),
        None => &request.text,
    };
    let Some(expression) = build_match(text) else {
        return browse(conn, request);
    };

    let mut values = vec![
        Value::Text(expression),
        Value::Real(weights.tags),
        Value::Real(weights.filename),
        Value::Real(weights.speech),
        Value::Real(weights.ocr),
    ];
    let filter = filters(request, &mut values, "f.");
    values.push(Value::Integer(request.limit as i64));

    let sql = format!(
        "SELECT f.id, f.path, f.hash, f.filename, f.kind, f.size, f.duration, f.state, f.mtime,
                bm25(fts, ?2, ?3, ?4, ?5) AS score,
                bm25(fts, 1.0, 0.0, 0.0, 0.0),
                bm25(fts, 0.0, 1.0, 0.0, 0.0),
                bm25(fts, 0.0, 0.0, 1.0, 0.0),
                bm25(fts, 0.0, 0.0, 0.0, 1.0),
                snippet(fts, 0, '', '', '…', 12),
                snippet(fts, 1, '', '', '…', 12),
                snippet(fts, 2, '', '', '…', 12),
                snippet(fts, 3, '', '', '…', 12)
         FROM fts JOIN files f ON f.id = fts.rowid
         WHERE fts MATCH ?1 AND f.missing_at IS NULL{filter}
         ORDER BY score
         LIMIT ?{}",
        values.len()
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        let scores = [row.get(10)?, row.get(11)?, row.get(12)?, row.get(13)?];
        let column = best_column(scores);
        let snippet: String = row.get(14 + column)?;
        let file = read_row(row)?;
        let matched = if snippet.trim().is_empty() {
            file.filename.clone()
        } else {
            snippet
        };
        Ok(Hit {
            file,
            score: row.get(9)?,
            signal: Signal::COLUMNS[column],
            matched,
            at_ms: None,
            // A column that did not match scores zero.
            named: scores[0] != 0.0 || scores[1] != 0.0,
        })
    })?;
    let mut hits: Vec<Hit> = rows.collect::<Result<_, _>>()?;
    for hit in &mut hits {
        find_moment(conn, hit, text)?;
    }
    Ok(hits)
}

/// The speech segment or the screen line that matched, as the time and the
/// text of the hit. The index leaves some screen lines out, so they cannot be
/// what matched.
fn find_moment(conn: &Connection, hit: &mut Hit, query: &str) -> Result<()> {
    let (at_ms, matched) = match hit.signal {
        Signal::Speech => {
            let spoken = segments::of_hash(conn, &hit.file.hash)?;
            match moment::best(&spoken, |s| &s.text, query) {
                Some(segment) => (Some(segment.start_ms), segment.text.clone()),
                None => return Ok(()),
            }
        }
        Signal::Ocr => {
            let mut lines = ocr::of_hash(conn, &hit.file.hash)?;
            lines.retain(|line| ocr::indexed(&line.text, line.edited));
            match moment::best(&lines, |line| &line.text, query) {
                Some(line) => (line.time_ms, line.text.clone()),
                None => return Ok(()),
            }
        }
        _ => return Ok(()),
    };
    // A still has one frame, so a time in it says nothing.
    hit.at_ms = at_ms.filter(|_| hit.file.kind != Kind::Still);
    hit.matched = matched.trim().to_string();
    Ok(())
}

/// What the grid shows when the query is empty: every file, in the order of
/// the sort.
pub fn browse(conn: &Connection, request: &Request) -> Result<Vec<Hit>> {
    let mut values = Vec::new();
    let filter = filters(request, &mut values, "");
    let order = match request.sort {
        Sort::Name => "",
        Sort::Recent => "mtime DESC, ",
        Sort::Size => "size DESC, ",
    };
    // Ties go by name, as in a folder.
    let sql = format!("{PRESENT}{filter} ORDER BY {order}lower(filename), path");

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        let file = read_row(row)?;
        Ok(Hit {
            matched: file.filename.clone(),
            file,
            score: 0.0,
            signal: Signal::Filename,
            at_ms: None,
            named: false,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The clauses for the chips and the folder, each with ` AND ` before it.
/// `table` is the alias of `files` with its dot, or nothing.
fn filters(request: &Request, values: &mut Vec<Value>, table: &str) -> String {
    let mut clauses = String::new();
    // There are three kinds, so a list of three is the same as no filter.
    if !request.kinds.is_empty() && request.kinds.len() < 3 {
        let mut places = Vec::new();
        for kind in &request.kinds {
            values.push(Value::Text(kind.as_str().to_string()));
            places.push(format!("?{}", values.len()));
        }
        clauses += &format!(" AND {table}kind IN ({})", places.join(", "));
    }
    if let Some(folder) = &request.within {
        values.push(Value::Text(files::folder_prefix(folder)));
        let at = values.len();
        clauses += &format!(" AND substr({table}path, 1, length(?{at})) = ?{at}");
    }
    clauses
}

/// The column that gave the best score. A column that did not match scores
/// zero, so the lowest score wins.
fn best_column(scores: [f64; 4]) -> usize {
    (0..scores.len())
        .min_by(|a, b| scores[*a].total_cmp(&scores[*b]))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(matched: &str, at_ms: Option<i64>) -> Hit {
        Hit {
            file: FileRow {
                id: 1,
                path: "C:/clips/a.mp4".into(),
                hash: "h".into(),
                filename: "a.mp4".into(),
                kind: Kind::Video,
                size: 1,
                duration: Some(60.0),
                state: files::State::Ready,
                mtime: 0,
            },
            score: 0.0,
            signal: Signal::Speech,
            matched: matched.into(),
            at_ms,
            named: false,
        }
    }

    #[test]
    fn a_moment_puts_its_time_before_the_match() {
        assert_eq!(
            hit("the line you said", Some(42_500)).snippet(),
            "0:42 the line you said"
        );
        assert_eq!(hit("", Some(7_000)).snippet(), "0:07");
        assert_eq!(hit("a.mp4", None).snippet(), "a.mp4");
    }

    #[test]
    fn the_strongest_column_gives_the_badge() {
        let column = best_column([0.0, -1.5, -0.2, 0.0]);
        assert_eq!(Signal::COLUMNS[column], Signal::Filename);
    }

    #[test]
    fn browse_lists_every_file_in_the_order_of_the_sort() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let files = [("b.mp4", 30, 5), ("A.mp4", 10, 5), ("c.mp4", 30, 900)];
        for (name, mtime, size) in files {
            let path = PathBuf::from(format!("C:/clips/{name}"));
            let scanned = files::Scanned {
                path: &path,
                hash: name,
                kind: Kind::Video,
                size,
                mtime,
            };
            files::upsert(db.conn(), &scanned).unwrap();
        }
        let names = |sort| {
            let request = Request::new("").limit(1).sort(sort);
            browse(db.conn(), &request)
                .unwrap()
                .into_iter()
                .map(|hit| hit.file.filename)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(Sort::Name), ["A.mp4", "b.mp4", "c.mp4"]);
        assert_eq!(names(Sort::Recent), ["b.mp4", "c.mp4", "A.mp4"]);
        assert_eq!(names(Sort::Size), ["c.mp4", "A.mp4", "b.mp4"]);
    }
}
