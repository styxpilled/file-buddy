//! Tags are the one kind of data the application cannot rebuild from the files.
//! They belong to the content, so a rename keeps them.

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, params, params_from_iter};

use crate::db::fts;

/// The tag id, creating the tag if it is new. Tags are lowercase.
fn ensure(conn: &Connection, name: &str) -> Result<i64> {
    let name = name.trim().to_lowercase();
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
    Ok(
        conn.query_row("SELECT id FROM tags WHERE name = ?1", [&name], |row| {
            row.get(0)
        })?,
    )
}

/// Attach one tag to several files at once. It is a transaction because each
/// file rebuilds its own index row: a failure halfway would leave some files
/// tagged, some indexed and no way to tell which.
pub fn attach(conn: &mut Connection, hashes: &[String], name: &str) -> Result<()> {
    let transaction = conn.transaction()?;
    let tag_id = ensure(&transaction, name)?;
    for hash in hashes {
        transaction.execute(
            "INSERT OR IGNORE INTO file_tags (hash, tag_id) VALUES (?1, ?2)",
            params![hash, tag_id],
        )?;
        fts::refresh_hash(&transaction, hash)?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn detach(conn: &mut Connection, hashes: &[String], name: &str) -> Result<()> {
    let transaction = conn.transaction()?;
    let name = name.trim().to_lowercase();
    for hash in hashes {
        transaction.execute(
            "DELETE FROM file_tags WHERE hash = ?1
             AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
            params![hash, &name],
        )?;
        fts::refresh_hash(&transaction, hash)?;
    }
    // A tag on no files is a name that can only clutter the autocomplete, and
    // that list is worth nothing if it is not the tags actually in use.
    transaction.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM file_tags)",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

/// The tags of every file in `from`, on `to` as well. An export gets the tags
/// of its sources before any scan finds it, and the scan indexes them.
pub fn copy(conn: &mut Connection, from: &[String], to: &str) -> Result<()> {
    let transaction = conn.transaction()?;
    for hash in from {
        transaction.execute(
            "INSERT OR IGNORE INTO file_tags (hash, tag_id)
             SELECT ?1, tag_id FROM file_tags WHERE hash = ?2",
            params![to, hash],
        )?;
    }
    fts::refresh_hash(&transaction, to)?;
    transaction.commit()?;
    Ok(())
}

/// Tags for the autocomplete. A tag that starts with what was typed comes
/// first, then the one used most, which is what stops `funny` and `funnny`
/// from both looking equally right.
pub fn matching(conn: &Connection, typed: &str, limit: usize) -> Result<Vec<String>> {
    let typed = escape_like(&typed.trim().to_lowercase());
    let mut statement = conn.prepare(
        r#"SELECT t.name
           FROM tags t LEFT JOIN file_tags ft ON ft.tag_id = t.id
           WHERE t.name LIKE '%' || ?1 || '%' ESCAPE '\'
           GROUP BY t.id
           ORDER BY (t.name LIKE ?1 || '%' ESCAPE '\') DESC, count(ft.hash) DESC, t.name
           LIMIT ?2"#,
    )?;
    let rows = statement.query_map(params![typed, limit as i64], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The tags every one of these files already has. Only these can be removed
/// from the whole marked set in one action.
pub fn common_to(conn: &Connection, hashes: &[String]) -> Result<Vec<String>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let places = (1..=hashes.len())
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
         WHERE ft.hash IN ({places})
         GROUP BY t.id HAVING count(DISTINCT ft.hash) = ?{}
         ORDER BY t.name",
        hashes.len() + 1
    );
    let mut values: Vec<rusqlite::types::Value> = hashes
        .iter()
        .map(|hash| rusqlite::types::Value::Text(hash.clone()))
        .collect();
    values.push(rusqlite::types::Value::Integer(hashes.len() as i64));

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// `%` and `_` are wildcards to LIKE, and a tag may hold either.
fn escape_like(text: &str) -> String {
    text.replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
         WHERE ft.hash = ?1 ORDER BY t.name",
    )?;
    let rows = statement.query_map([hash], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The tags of every file that has one, by hash, each list by name.
pub fn by_hash(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut statement = conn.prepare(
        "SELECT ft.hash, t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
         ORDER BY ft.hash, t.name",
    )?;
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for row in rows {
        let (hash, name) = row?;
        tags.entry(hash).or_default().push(name);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn seed(db: &Db, hash: &str, filename: &str) {
        db.conn()
            .execute(
                "INSERT INTO files (path, hash, filename, kind, size, mtime, seen_at)
                 VALUES (?1, ?2, ?3, 'video', 1, 1, 1)",
                params![filename, hash, filename],
            )
            .unwrap();
    }

    #[test]
    fn an_export_gets_the_tags_of_one_source() {
        let mut db = Db::open_in_memory().unwrap();
        attach(db.conn_mut(), &["src".into()], "cat").unwrap();
        attach(db.conn_mut(), &["src".into()], "funny").unwrap();
        attach(db.conn_mut(), &["other".into()], "dog").unwrap();

        copy(db.conn_mut(), &["src".into()], "out").unwrap();
        assert_eq!(of_hash(db.conn(), "out").unwrap(), ["cat", "funny"]);
        assert_eq!(of_hash(db.conn(), "src").unwrap(), ["cat", "funny"]);
    }

    #[test]
    fn a_sequence_export_gets_the_union_of_its_stills() {
        let mut db = Db::open_in_memory().unwrap();
        attach(db.conn_mut(), &["a".into(), "b".into()], "cat").unwrap();
        attach(db.conn_mut(), &["a".into()], "funny").unwrap();
        attach(db.conn_mut(), &["c".into()], "dog").unwrap();
        attach(db.conn_mut(), &["out".into()], "kept").unwrap();

        let stills = ["a".into(), "b".into(), "c".into(), "untagged".into()];
        copy(db.conn_mut(), &stills, "out").unwrap();
        assert_eq!(
            of_hash(db.conn(), "out").unwrap(),
            ["cat", "dog", "funny", "kept"]
        );
    }

    #[test]
    fn copied_tags_reach_the_index_of_a_file_already_found() {
        let mut db = Db::open_in_memory().unwrap();
        seed(&db, "out", "export.mp4");
        attach(db.conn_mut(), &["src".into()], "reaction").unwrap();
        copy(db.conn_mut(), &["src".into()], "out").unwrap();
        let found: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM fts WHERE fts MATCH 'tags:reaction'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, 1);
    }

    #[test]
    fn one_tag_reaches_every_marked_file_and_the_index() {
        let mut db = Db::open_in_memory().unwrap();
        seed(&db, "aaa", "one.mp4");
        seed(&db, "bbb", "two.mp4");

        let hashes = vec!["aaa".to_string(), "bbb".to_string()];
        attach(db.conn_mut(), &hashes, "  Reaction  ").unwrap();

        // Lowercased on the way in, so one tag cannot become three.
        assert_eq!(of_hash(db.conn(), "aaa").unwrap(), vec!["reaction"]);
        assert_eq!(of_hash(db.conn(), "bbb").unwrap(), vec!["reaction"]);

        let found: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM fts WHERE fts MATCH 'tags:reaction'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, 2);

        detach(db.conn_mut(), &hashes, "REACTION").unwrap();
        assert!(of_hash(db.conn(), "aaa").unwrap().is_empty());
        // Taking it off the last file takes the name out of the autocomplete.
        assert!(matching(db.conn(), "reaction", 10).unwrap().is_empty());
    }

    #[test]
    fn the_autocomplete_puts_a_prefix_first_then_the_most_used() {
        let mut db = Db::open_in_memory().unwrap();
        seed(&db, "aaa", "one.mp4");
        seed(&db, "bbb", "two.mp4");

        // `unfunny` holds `funny` but does not start with it. `funnny` is the
        // typo the autocomplete exists to prevent.
        attach(db.conn_mut(), &["aaa".into(), "bbb".into()], "unfunny").unwrap();
        attach(db.conn_mut(), &["aaa".into()], "funnny").unwrap();
        ensure(db.conn(), "funny").unwrap();

        let found = matching(db.conn(), "funn", 10).unwrap();
        assert_eq!(found, vec!["funnny", "funny", "unfunny"]);
    }

    #[test]
    fn a_wildcard_in_the_query_is_text_not_syntax() {
        let db = Db::open_in_memory().unwrap();
        ensure(db.conn(), "a%b").unwrap();
        ensure(db.conn(), "axb").unwrap();

        assert_eq!(matching(db.conn(), "a%b", 10).unwrap(), vec!["a%b"]);
    }

    #[test]
    fn only_a_tag_on_every_file_is_common_to_them() {
        let mut db = Db::open_in_memory().unwrap();
        seed(&db, "aaa", "one.mp4");
        seed(&db, "bbb", "two.mp4");

        attach(db.conn_mut(), &["aaa".into(), "bbb".into()], "both").unwrap();
        attach(db.conn_mut(), &["aaa".into()], "one only").unwrap();

        let hashes = vec!["aaa".to_string(), "bbb".to_string()];
        assert_eq!(common_to(db.conn(), &hashes).unwrap(), vec!["both"]);
        assert_eq!(
            common_to(db.conn(), &["aaa".to_string()]).unwrap(),
            vec!["both", "one only"]
        );
    }
}
