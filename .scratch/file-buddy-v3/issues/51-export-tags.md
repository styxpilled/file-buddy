# 51 - Tags carry over to exports

Status: done

Depends on 36. Read the Tags carry over section of the v3 spec first.

## Work

When an export is written, hash it with `media::hash::file_hash` and write the
source's tags for that hash through `db/tags.rs`. Do not wait for a scan. A
sequence export gets the tags of every still in it.

## Tests

Tags of one source, and the union of several.

## Comments

Done. The Studio keeps the hashes of the files an edit came from: the
selection, or every marked still of a sequence. They are taken when the edit
opens, from the index, so no source is hashed again. The export thread
hashes the new file with `media::hash::file_hash` once FFmpeg is done, and
`tags::copy` writes the union of the sources' tags for that hash and
refreshes its index row, if a scan already found it. A failure to hash or to
tag is logged. The export itself still counts as done.

A skipped or deleted frame of a sequence still gives its tags: the strip does
not keep which still a frame came from.

Tests: one source, the union of several with a tag the export already had,
and the index row of an export a scan already found. Not checked in the
window: that needs an export into the real export folder from a tagged file.
