# MEMORY

Current goals:
- Stabilize core runtime.
- Keep module lifecycle reliable (install/enable/update/hot-reload).

Known constraints:
- Default behavior should stay conservative.
- Features requiring external services must degrade gracefully.

Recent decisions:
- Core/module split enforced.
- STT remains optional but in-core.
