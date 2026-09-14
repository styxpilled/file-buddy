# 06 - Tags

Status: ready-for-agent

Depends on 04.

## Work

- A key opens a tag input on the selected file, with autocomplete over the tags
  that already exist. Enter applies. Escape closes.
- Tags are lowercase. Autocomplete is what stops `funny`, `Funny` and `funnny`
  from becoming three tags and making the feature useless.
- Several selected files can be tagged at once.
- Tags feed the highest weighted FTS column, so a bare word finds them with no
  syntax.
- The `tag:` operator limits a term to tags, for a common word that filename
  matches would otherwise bury.

## Care

Tags exist only in the database. There is no export. They are the only data the
application cannot rebuild from the files on disk, so no migration may drop or
recreate the tags tables.
