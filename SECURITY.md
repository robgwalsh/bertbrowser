# Security Policy

BertBrowser is a personal project maintained by one person in his spare time. That shapes what
follows: the reporting channel is real and monitored, but response times are best-effort, and there
is no bounty.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Use GitHub's private vulnerability reporting:
[**Report a vulnerability**](https://github.com/robgwalsh/bertbrowser/security/advisories/new).
It is private between you and the maintainer until an advisory is published.

Useful things to include: the version (Help/About, or the folder name under
`%LOCALAPPDATA%\BertBrowser`), what you did, what happened, and — if it matters to the finding —
whether the account you ran as was an administrator.

What to expect: an acknowledgement within about a week, and a fix shipped as a normal release with
an advisory credited to you unless you would rather stay anonymous. If a report turns out to be one
of the accepted risks below, you will get a straight answer saying so rather than silence.

## Supported versions

Only the **latest release** is supported. Installed copies update themselves from GitHub Releases on
launch, so "upgrade to the current version" is the fix for anything already patched. Older versions
receive nothing.

## Security model — please read before reporting

Some of BertBrowser's behaviour looks alarming and is intentional. These are known, accepted, and
documented; reports of them will be closed as "by design" unless they come with an angle not covered
here.

### The app runs as Administrator. What it launches does not.

BertBrowser requests `requireAdministrator` in its manifest. It needs a raw volume handle to read
the NTFS Master File Table and the USN change journal, which is what makes global search instant;
Windows grants that only to an elevated process. That is the *only* thing needing it.

An elevated process normally passes its token to everything it starts, with no prompt — so a file
browser like this one would silently run a downloaded program as administrator. BertBrowser does not
do that:

> **Anything you open from BertBrowser runs as your ordinary user account, exactly as if you had
> double-clicked it in Explorer — because Explorer is what starts it.**

The mechanism: `explorer.exe` is already running at your normal integrity level and publishes its
automation object system-wide. BertBrowser reaches that object and asks *it* to perform the launch,
so the new process is Explorer's child and inherits Explorer's ordinary token rather than
BertBrowser's administrator one. Double-clicking a file, "Open in Terminal", "Open in VS Code",
opening a portable device, and custom commands all go through this single path.

The same indirection is what makes elevation honest. Asking for the `runas` verb from BertBrowser
itself would elevate silently — it already holds the token, so there is nothing to consent to. Asked
from medium-integrity Explorer it is a real elevation request, and Windows shows the consent dialog.

There are three explicit ways to elevate, and each one prompts:

- **Run as administrator** in the file list's right-click menu.
- **Ctrl+Shift** while double-clicking, or Ctrl+Shift+Enter — Explorer's own convention.
- **Run as administrator** on an individual custom command (Settings → Commands). Commands carrying
  it are marked with a shield in the menu.

**The refusal contract:** if the desktop shell cannot be reached at all — Explorer is not running,
or this session uses a different shell — BertBrowser does **not** quietly fall back to launching the
thing itself, because that would mean launching it as administrator. It stops, explains, and asks.
Declining starts nothing. If the shell is reached but does not answer in time, BertBrowser reports
that and stops rather than retrying, since a retry could start the same thing twice.

Limits worth knowing:

- **A de-elevated child cannot read what only an administrator can.** Opening a file under
  `System32\config` from BertBrowser will fail for the child even though BertBrowser can list the
  folder. That is correct, and it is what "Run as administrator" is there for.
- **If your desktop shell is itself elevated** (UAC turned off, or signed in as the built-in
  Administrator), nothing on the machine is de-elevated and neither is this. BertBrowser still
  matches what double-clicking in Explorer does, which is the promise being made.
- **A program that requests elevation itself** now raises its own UAC prompt, where previously it
  inherited administrator rights silently.

### Dragging files out of BertBrowser

Files can be dragged from BertBrowser into other applications. If the receiving application takes
the drop as a **move**, the CF_HDROP contract makes the source responsible for removing the
originals — so BertBrowser does, but only through its ordinary reversible delete. The delete
planner's refusals still apply, so no external window can talk BertBrowser into removing a drive
root or a protected system folder, and Ctrl+Z puts everything back.

BertBrowser asks for a **copy** by default, so a drag into another application adds a copy there
rather than taking the file out of the folder you were looking at. Holding Shift still requests a
move, as it does everywhere in Windows.

Dropping files *into* BertBrowser from other applications is deliberately **not** supported. Windows
blocks it because BertBrowser runs elevated, and the workaround would mean accepting a channel from
lower-integrity processes for no benefit this app needs.

### Custom commands run what you tell them to

User-defined context-menu commands (Settings → Commands) execute the program you name with the
arguments you write, once per selected file. This is the feature working. They run as your ordinary
user unless you tick **Run as administrator** on that command. Arguments are substituted verbatim
into your template, so quote `"{path}"` when a path may contain spaces.

The program name is resolved to a full path against `PATH` before anything is launched — never
against the folder you happen to be browsing — so what runs is decided by your `PATH`, not by a file
sitting next to the one you clicked.

### Deleted files are moved, not erased

An ordinary delete sends items to the **Windows Recycle Bin**, where they stay — visible and
restorable in Explorer — until you empty it. Ctrl+Z restores from there.

Where a volume has no working Recycle Bin (a network share, removable media with it turned off) the
items go instead to a hidden `.bertbrowser-trash` folder at the root of their volume, and are erased
when the undo slot is retired or on a startup sweep of batches over a day old. Two things follow on
a **shared** computer for that fallback path only: other local users can see the *names* of what you
deleted, and can tamper with the staged data (destroying your undo). File contents keep their
original permissions across the move either way.

Shift+Delete erases in place, holds nothing, and cannot be undone.

Because this app is elevated, one Windows dialog is deliberately left switched on: if an item cannot
be recycled — most often because it is larger than the bin's quota — Windows asks before erasing it
rather than being silently permitted to. Every other shell confirmation, progress and error dialog
is suppressed in favour of BertBrowser's own.

### What *is* in scope

Reports along these lines are wanted:

- A path that escapes the folder it should be confined to — theme ids, staging folders, index keys.
- Anything that makes BertBrowser write, delete, or move a file the user did not choose, especially
  where being elevated turns it into a privilege escalation.
- A way to get code running through BertBrowser without the user launching it — parsing untrusted
  input (theme JSON, MFT records, settings) into something executable.
- **A launch that reaches its target carrying BertBrowser's administrator token when the user did
  not ask for it** — a path through the launcher that skips the shell, or a case where the
  refuse-and-ask contract above does not hold.
- A way to make BertBrowser resolve a program name to somewhere an attacker can write.
- The update path: a way to make an installed copy fetch or apply something other than a genuine
  release.

## Dependencies

`Directory.Build.props` sets `TreatWarningsAsErrors`, and NuGet's audit surfaces known-vulnerable
packages as warnings — so a dependency with a published advisory **fails the build** rather than
shipping quietly. If you are contributing and hit `NU1903`, that is the guard working; bump or pin
the package rather than suppressing it.
