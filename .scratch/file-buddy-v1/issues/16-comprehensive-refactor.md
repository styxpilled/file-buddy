# 16 - Comprehensive refactor

Status: ready-for-agent

Depends on 14 and 15. Clean up and refactor the entire codebase, leaving the repository in a good state for future development. Much code was added after issue 12.

The behavior the user sees does not change. Internal behavior can change where the result is better.

Out of scope: the vendored `gpui-video`.

After this issue, we reevaluate the project goals for v2.

## Work

- Analyze the entire codebase.
- Reflect on necessity and maintainability and make decisions based on those. Think about various approaches - your first idea might not be the best one.
- Simplify unnecessary complexity.
