# AGENTS.md

Supplements `CLAUDE.md` (the primary project instructions).

## Do not launch the app to test

Do not build-and-run BertBrowser to verify changes (no launching `BertBrowser.exe`,
no UI-driving/screenshot verification) unless the user explicitly asks for it.
Rely on `dotnet build` and `dotnet test` instead. The user runs and eyeballs the
app themselves.
