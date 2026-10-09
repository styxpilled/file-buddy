# 47 - Ctrl-click on a kind chip

Status: ready-for-agent

Depends on nothing. Read the Kind chips section of the v3 spec first.

## Work

A ctrl-click on a chip turns that kind on and the others off. A ctrl-click on
the only chip that is on turns every kind on. `toggle_kind` is in
`src/ui/mod.rs`.

## Tests

Both cases, and a ctrl-click on a chip that is off.

## Comments
