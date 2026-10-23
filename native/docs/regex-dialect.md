# Regular expressions in the native build

Two user-facing surfaces take a regular expression: the `regex:` search term
(`bb-core::search::grammar`) and a rename rule in regex mode (`bb-core::rename`, M2). The C# app
used .NET `System.Text.RegularExpressions`; the native build uses the `fancy-regex` crate (0.19),
which layers backtracking features over the linear-time `regex` crate. Patterns that need none of
the fancy features run in guaranteed linear time, which is why the catastrophic-pattern test
(`(a+)+$` against a long run of `a`) passes without a time budget rather than with one.

## What carries over unchanged

Literals, classes, `.`, anchors, `\b`, `\d \w \s` and their negations, Unicode categories
(`\p{L}`), greedy and lazy quantifiers, `{n,m}`, alternation, non-capturing groups `(?:…)`, named
groups in both spellings `(?<name>…)` and `(?P<name>…)`, backreferences `\1` and `\k<name>`,
lookahead `(?=…)` `(?!…)`, lookbehind `(?<=…)` `(?<!…)`, atomic groups `(?>…)`, inline flags
`(?i)` `(?m)` `(?s)` `(?x)`.

Case-insensitive matching is always on for the search term (as in C#) and follows the rename rule's
"match case" switch. The native build folds by Unicode simple case folding; .NET folded by the
invariant culture's lower-casing. They agree on every script the app has been used with; the one
known divergence class is a character whose fold and lower-case differ (the Kelvin sign `K` matches
`k` here and did not in C#).

## Replacement strings (rename)

Expansion is implemented in `bb-core`, not by the engine, so the .NET spellings keep working:
`$1`, `${1}`, `${name}`, `$$` for a literal dollar, and `$0` for the whole match. Not supported:
`$&`, `` $` ``, `$'`, `$+`, `$_`. A `$` followed by anything else is kept literally, which is what
.NET did for an unknown group.

## Not supported (fails to parse, shown as an invalid pattern)

- Conditionals `(?(name)yes|no)` and balancing groups `(?<open-close>…)`.
- `\G`, `(?#comment)`, `\e`, `\cX`, octal escapes `\012`.
- .NET block names `\p{IsGreek}`; use the script name `\p{Greek}`.
- `RegexOptions.ECMAScript` and `RightToLeft` have no counterpart; nothing in the app set them.
- Possessive quantifiers (`a*+`): write an atomic group instead (`(?>a*)`).

## Limits

`fancy-regex` is given a backtrack limit (`RegexBuilder::backtrack_limit`) rather than a wall-clock
budget, so an expensive pattern fails deterministically with "regex too complex" instead of taking
a different time on every machine. The search grammar reports that the same way it reports a parse
error: the term matches nothing and the status line says why.
