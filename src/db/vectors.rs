//! What the frames of a file show: one CLIP vector per sampled frame, keyed by
//! content hash, with the time of the frame.

use anyhow::{Result, bail};
use rusqlite::{Connection, params};

use crate::clip::Frame;

/// Replace every vector of these bytes.
pub fn replace(conn: &Connection, hash: &str, frames: &[Frame]) -> Result<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM vectors WHERE hash = ?1", [hash])?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO vectors (hash, time_ms, vector) VALUES (?1, ?2, ?3)")?;
        for frame in frames {
            insert.execute(params![hash, frame.time_ms, to_blob(&frame.vector)])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// Every vector written after row `after`, in the order written, with its row
/// id and hash. Search loads them all once, then only the new ones.
pub fn since(conn: &Connection, after: i64) -> Result<Vec<(i64, String, Frame)>> {
    let mut statement =
        conn.prepare("SELECT id, hash, time_ms, vector FROM vectors WHERE id > ?1 ORDER BY id")?;
    let rows = statement.query_map([after], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    let mut all = Vec::new();
    for row in rows {
        let (id, hash, time_ms, blob) = row?;
        all.push((
            id,
            hash,
            Frame {
                time_ms,
                vector: from_blob(&blob)?,
            },
        ));
    }
    Ok(all)
}

/// Little-endian `f32`s.
fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(blob: &[u8]) -> Result<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        bail!("a vector of {} bytes is not whole floats", blob.len());
    }
    Ok(blob
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn frame(time_ms: i64, vector: &[f32]) -> Frame {
        Frame {
            time_ms,
            vector: vector.to_vec(),
        }
    }

    #[test]
    fn a_vector_comes_back_as_it_went_in() {
        let vector = [0.6, -0.8, f32::MIN_POSITIVE, 1.0e-7];
        assert_eq!(from_blob(&to_blob(&vector)).unwrap(), vector);
    }

    #[test]
    fn a_blob_of_the_wrong_length_is_an_error() {
        assert!(from_blob(&[0, 0, 0]).is_err());
    }

    #[test]
    fn replace_keeps_only_the_new_frames() {
        let db = Db::open_in_memory().unwrap();
        replace(db.conn(), "h", &[frame(0, &[1.0]), frame(500, &[0.5])]).unwrap();
        replace(db.conn(), "h", &[frame(1000, &[0.25])]).unwrap();
        replace(db.conn(), "g", &[frame(0, &[-1.0])]).unwrap();
        let all: Vec<_> = since(db.conn(), 0)
            .unwrap()
            .into_iter()
            .map(|(_, hash, frame)| (hash, frame))
            .collect();
        assert_eq!(
            all,
            vec![
                ("h".to_string(), frame(1000, &[0.25])),
                ("g".to_string(), frame(0, &[-1.0])),
            ]
        );
        let (last, _, _) = since(db.conn(), 0).unwrap()[0].clone();
        assert_eq!(since(db.conn(), last).unwrap().len(), 1);
    }
}
