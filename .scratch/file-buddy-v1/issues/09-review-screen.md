# 09 - Review screen

Status: ready-for-agent

Depends on 03 and 08. Four kinds of file need a person to look at them, and they
all want the same screen: a list, a preview and a key to open Explorer.

## Groups

- Duplicate files, grouped by content hash. The hash makes this free.
- Files longer than 5 minutes. They are outliers. The user deletes them outside
  the application.
- Files that could not be indexed, with the error text.
- Files with no proper name. This tab is the bulk walk from issue 08.

## Rules

The application never deletes a file. It shows the file and opens Explorer. The
decision and the action stay with the user.
