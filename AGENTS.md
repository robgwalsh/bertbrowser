# AGENTS.md

Supplements `CLAUDE.md` (the primary project instructions).

## Do not launch the app to test

Do not build-and-run BertBrowser to verify changes (no launching `BertBrowser.exe`,
no UI-driving/screenshot verification) unless the user explicitly asks for it.
Rely on `dotnet build` and `dotnet test` instead. The user runs and eyeballs the
app themselves.

`BertBrowser.exe` stays off-limits **whatever the reason** — it puts a window over
whatever the user is doing and takes their keyboard. When they *do* ask for the
interface to be checked, `tools/BertBrowser.Harness` is the way: it hosts the same
window offscreen, refused activation, and hands back PNGs. See
`.claude/skills/verify`.
