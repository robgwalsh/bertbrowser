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

### The app runs as you. One helper process runs as Administrator.

BertBrowser's manifest is `asInvoker`: it runs with your ordinary account, and so does everything it
opens. There is one exception, and it is a separate program.

Reading the NTFS Master File Table and the USN change journal — what makes global search instant —
needs a raw volume handle, and Windows grants that only to an elevated process. So that one job
lives in **`BertBrowser.Indexer.exe`**, which ships beside the app and is started on demand:

> **One UAC prompt, for the index helper, when BertBrowser starts. Declining costs you instant
> whole-PC search and nothing else — the app is fully usable without it, and the status bar offers
> to ask again.**

What the helper can be asked to do is deliberately tiny. The app and the helper talk over a named
pipe whose entire vocabulary in the privileged direction is four words — *hello*, *start*,
*shutdown*, *ping*. **None of them carries a path**, a filename or a program name, and the protocol
rejects any message that tries to attach one. There is no way to talk the elevated process into
touching a file of your choosing, because it does not accept the concept.

Other properties worth knowing:

- **The helper dies with the app.** Losing the pipe is what tells it to exit, so it goes when
  BertBrowser does, crash included. It is also watching the app's process handle as a backstop.
- **The app cannot kill the helper**, since an ordinary process may not terminate an elevated one.
  That is why the two mechanisms above are the guarantee rather than a courtesy shutdown message.
- **The pipe is created by the app, not the helper**, and admits only your own account. Both ends
  additionally check that the peer is the process they expect.
- **Nothing retries by itself.** A retry means another UAC prompt, so it only happens when you click.

Because the app is no longer elevated, launching is now unremarkable: opening a file starts it with
your ordinary token, the way Explorer does, with no indirection needed. Elevating on purpose is
still offered three ways, and each prompts:

- **Run as administrator** in the file list's right-click menu.
- **Ctrl+Shift** while double-clicking, or Ctrl+Shift+Enter — Explorer's own convention.
- **Run as administrator** on an individual custom command (Settings → Commands). Commands carrying
  it are marked with a shield in the menu.

Limits worth knowing:

- **Folders that need administrator rights are now closed to BertBrowser**, exactly as they are to
  Explorer: `System Volume Information`, another account's profile, and similar. Listing one reports
  access denied rather than showing its contents. This is the intended trade.
- **Whole-PC search can find files the browser cannot open.** The helper reads the master file table,
  which covers the whole volume, so a search may name a file that listing its folder would refuse.
  Explorer's index behaves the same way.
- **Deleting or moving inside protected system folders now fails** where it previously succeeded.
  The failure is reported per item; the rest of the batch still runs.

### Dragging files out of BertBrowser

Files can be dragged from BertBrowser into other applications. If the receiving application takes
the drop as a **move**, the CF_HDROP contract makes the source responsible for removing the
originals — so BertBrowser does, but only through its ordinary reversible delete. The delete
planner's refusals still apply, so no external window can talk BertBrowser into removing a drive
root or a protected system folder, and Ctrl+Z puts everything back.

BertBrowser asks for a **copy** by default, so a drag into another application adds a copy there
rather than taking the file out of the folder you were looking at. Holding Shift still requests a
move, as it does everywhere in Windows.

Dropping files *into* BertBrowser from other applications is **not supported yet**. It used to be
impossible — Windows blocked it while the app ran elevated — and now that the app runs as you the
block is gone, but nothing has been built to handle such a drop. An external drop is ignored rather
than acted on. Dragging files *out* is unaffected.

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

One Windows dialog is deliberately left switched on: if an item cannot be recycled — most often
because it is larger than the bin's quota — Windows asks before erasing it rather than being
silently permitted to. That is the one case pre-flight cannot predict, which is why it is the one
confirmation left to the shell. Every other shell confirmation, progress and error dialog is
suppressed in favour of BertBrowser's own.

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
