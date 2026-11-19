# Release notes

`release-notes/<version>.md` (for example `1.2.0.md`, or `1.3.0-beta.1.md`) becomes the GitHub release's notes.
Write it before `just ship`; `just notes patch` (or `minor`, `beta`, a version) drafts it with a cheap model from the commits since the last release, for you to read and edit. Without one, the notes are
generated from the commits since the last release.
