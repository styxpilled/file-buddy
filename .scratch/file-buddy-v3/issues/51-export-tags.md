# 51 - Tags carry over to exports

Status: ready-for-agent

Depends on 36. Read the Tags carry over section of the v3 spec first.

## Work

When an export is written, hash it with `media::hash::file_hash` and write the
source's tags for that hash through `db/tags.rs`. Do not wait for a scan. A
sequence export gets the tags of every still in it.

## Tests

Tags of one source, and the union of several.

## Comments
