//! One FTS5 query over four columns, weighted by the config. `query` turns what
//! the user typed into the expression this runs.

pub mod folder;
pub mod query;

use std::path::PathBuf;

use anyhow::Result;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use self::query::build_match;
use crate::config::Weights;
use crate::db::files::{self, FileRow};
use crate::media::Kind;

/// Which column gave a result its best score. The grid shows it as a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Tags,
    Filename,
    Speech,
    Ocr,
}

impl Signal {
    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Tags => "tag",
            Signal::Filename => "filename",
            Signal::Speech => "speech",
            Signal::Ocr => "screen text",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub file: FileRow,
    /// The bm25 score. Lower is better, and it is always negative.
    pub score: f64,
    pub signal: Signal,
    /// The text that matched, from the column the badge names.
    pub matched: String,
}

/// What to search for. The chips decide `kinds`.
#[derive(Debug, Clone)]
pub struct Request {
    pub text: String,
    /// The kinds the chips allow. Empty means every kind.
    pub kinds: Vec<Kind>,
    /// Only files under this folder, at any depth.
    pub within: Option<PathBuf>,
    pub limit: usize,
}

impl Request {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kinds: Vec::new(),
            within: None,
            limit: 500,
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
}

/// The best results for what the user typed.
pub fn search(conn: &Connection, request: &Request, weights: &Weights) -> Result<Vec<Hit>> {
    let Some(expression) = build_match(&request.text) else {
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
        let per_column = [
            (Signal::Tags, row.get::<_, f64>(10)?),
            (Signal::Filename, row.get::<_, f64>(11)?),
            (Signal::Speech, row.get::<_, f64>(12)?),
            (Signal::Ocr, row.get::<_, f64>(13)?),
        ];
        let signal = best_signal(&per_column);
        let snippet: String = row.get(14 + signal_index(signal))?;
        let file = read_file(row)?;
        let matched = if snippet.trim().is_empty() {
            file.filename.clone()
        } else {
            snippet
        };
        Ok(Hit {
            file,
            score: row.get(9)?,
            signal,
            matched,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// What the grid shows when the query is empty: the newest files first.
pub fn browse(conn: &Connection, request: &Request) -> Result<Vec<Hit>> {
    let mut values = Vec::new();
    let filter = filters(request, &mut values, "");
    values.push(Value::Integer(request.limit as i64));
    let sql = format!(
        "SELECT id, path, hash, filename, kind, size, duration, state, mtime
         FROM files WHERE missing_at IS NULL{filter} ORDER BY mtime DESC LIMIT ?{}",
        values.len()
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        let file = read_file(row)?;
        Ok(Hit {
            matched: file.filename.clone(),
            file,
            score: 0.0,
            signal: Signal::Filename,
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

fn read_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    let kind: String = row.get(4)?;
    Ok(FileRow {
        id: row.get(0)?,
        path: row.get::<_, String>(1)?.into(),
        hash: row.get(2)?,
        filename: row.get(3)?,
        kind: kind.parse().unwrap_or(Kind::Still),
        size: row.get::<_, i64>(5)? as u64,
        duration: row.get(6)?,
        state: row.get(7)?,
        mtime: row.get(8)?,
    })
}

fn signal_index(signal: Signal) -> usize {
    match signal {
        Signal::Tags => 0,
        Signal::Filename => 1,
        Signal::Speech => 2,
        Signal::Ocr => 3,
    }
}

/// A column that did not match scores zero, so the lowest score wins.
fn best_signal(scores: &[(Signal, f64); 4]) -> Signal {
    scores
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(signal, _)| *signal)
        .unwrap_or(Signal::Filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strongest_column_gives_the_badge() {
        let scores = [
            (Signal::Tags, 0.0),
            (Signal::Filename, -1.5),
            (Signal::Speech, -0.2),
            (Signal::Ocr, 0.0),
        ];
        assert_eq!(best_signal(&scores), Signal::Filename);
    }
}
