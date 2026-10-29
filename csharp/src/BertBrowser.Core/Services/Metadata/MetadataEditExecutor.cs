using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Carries out a <see cref="MetadataEditPlan"/> by rewriting each file beside itself, checking the
/// rewrite, and swapping it in.
/// </summary>
/// <remarks>
/// <para>
/// <b>The order is the safety</b>, as it is for an archive edit. Write a sibling; read it back and
/// require that it holds what was asked for <em>and</em> that everything which is not metadata is
/// byte-for-byte what the original had; only then swap. Until the swap the file on disk is the one
/// the user started with, so a cancel, a crash or a codec bug costs a temporary file.
/// </para>
/// <para>
/// <b>The original is held, not deleted</b>, under <see cref="ReplacedMarker"/>, until the undo
/// history retires the edit. An undo swaps it back and holds the edited file in turn, under
/// <see cref="EditedMarker"/>. Both directions are the same swap, which is why there is one
/// <see cref="Swap"/> and not an undo and a redo.
/// </para>
/// <para>
/// One file's failure never affects the others, and nothing here is retried elevated: a tag parser
/// reads bytes somebody else wrote, which is not something to do with an administrator's token.
/// </para>
/// </remarks>
public sealed class MetadataEditExecutor
{
    /// <summary>Marks a file being rewritten, before it is swapped in.</summary>
    public const string RewriteMarker = ".bertbrowser-rewrite-";

    /// <summary>Marks a replaced original while the edit can still be undone.</summary>
    public const string ReplacedMarker = ".bertbrowser-replaced-";

    /// <summary>Marks an edited file an undo set aside, while it can still be redone.</summary>
    public const string EditedMarker = ".bertbrowser-edited-";

    private readonly IMetadataProbe _probe;
    private readonly IPropertyFallback? _fallback;

    public MetadataEditExecutor(IMetadataProbe probe, IPropertyFallback? fallback = null)
    {
        _probe = probe;
        _fallback = fallback;
    }

    public MetadataEditExecutor() : this(new FileSystemMetadataProbe())
    {
    }

    public MetadataEditOutcome Execute(
        MetadataEditPlan plan,
        CancellationToken ct = default,
        IProgress<TransferProgress>? progress = null,
        PauseGate? pause = null)
    {
        if (!plan.HasWork) return MetadataEditOutcome.Empty;

        var run = new ProgressCoalescer(ct, progress, plan.Files.Count, pause);
        var edited = new List<HeldVersion>();
        var failed = new List<FailedMetadataEdit>();
        var unchanged = new List<string>();
        var cancelled = false;

        foreach (var file in plan.Files)
        {
            if (ct.IsCancellationRequested)
            {
                cancelled = true;
                break;
            }

            run.BeginItem(Path.GetFileName(file.Path));
            run.BeginFile(file.Length);

            string? rewrite = null;
            try
            {
                rewrite = StagedReplace.Beside(file.Path, RewriteMarker);
                var held = file.ViaWindows ? EditViaWindows(file, rewrite, ct) : EditOne(file, rewrite, ct);
                if (held is not null) edited.Add(held);
                else unchanged.Add(file.Path);
            }
            catch (OperationCanceledException)
            {
                TryDelete(rewrite);
                cancelled = true;
                break;
            }
            catch (MetadataFormatException ex)
            {
                TryDelete(rewrite);
                failed.Add(new FailedMetadataEdit(file.Path, ex.Message));
            }
            catch (Exception ex) when (IsEditFailure(ex))
            {
                TryDelete(rewrite);
                failed.Add(new FailedMetadataEdit(file.Path,
                    $"{Path.GetFileName(file.Path)} could not be changed: {ex.Message}",
                    AccessDenied.Caused(ex)));
            }

            run.EndFile();
            run.EndItem();
        }

        run.Finished();
        return new MetadataEditOutcome(edited, failed, cancelled) { Unchanged = unchanged };
    }

    /// <summary>Puts the originals back, holding each edited file so the edit can be redone.</summary>
    public MetadataSwapResult Undo(IReadOnlyList<HeldVersion> edited) => Swap(edited, EditedMarker);

    /// <summary>Takes undone edits back in, holding each original again.</summary>
    public MetadataSwapResult Redo(IReadOnlyList<HeldVersion> restored) => Swap(restored, ReplacedMarker);

    /// <summary>
    /// Erases held versions once the step that needs them can no longer be taken — when the undo
    /// history releases the entry, or the session ends.
    /// </summary>
    public static void CommitStaging(IReadOnlyList<HeldVersion> held)
    {
        foreach (var version in held)
        {
            // Named the way this class names them, or it is not ours to erase.
            if (!StagedReplace.IsMarked(version.HeldPath, ReplacedMarker) &&
                !StagedReplace.IsMarked(version.HeldPath, EditedMarker))
                continue;

            TryDelete(version.HeldPath);
        }
    }

    /// <returns>The file with its original held, or null when the edit turned out to change
    /// nothing — a location removed from a picture that had none.</returns>
    private HeldVersion? EditOne(PlannedMetadataEdit file, string rewrite, CancellationToken ct)
    {
        var path = file.Path;

        // The planner decided this file could be edited; disk is the authority.
        if (MetadataEditPlanner.Refusal(path, _probe.AttributesOf(path)) is { } refusal)
            throw new MetadataFormatException(refusal.Message);

        var codec = MetadataCodecs.For(path)!;
        var original = EntryStamp.Of(path) ?? throw Gone(path);
        var identical = false;

        using (var source = ReadOnlyFile.TryOpen(path) ?? throw new IOException("it could not be opened."))
        using (var destination = new FileStream(
                   rewrite, FileMode.CreateNew, FileAccess.ReadWrite, FileShare.None,
                   bufferSize: 128 * 1024))
        {
            var before = codec.PayloadDigest(source);
            var edit = Resolve(file.Edit, codec, source, path);

            source.Position = 0;
            codec.Write(source, destination, edit, ct);
            destination.Flush();

            // Verified before anything irreversible happens.
            destination.Position = 0;
            var written = codec.Read(destination);
            foreach (var (field, value) in edit.Changes)
            {
                if (!string.Equals(written.Get(field), value, StringComparison.Ordinal))
                    throw new MetadataFormatException(
                        $"{MetadataFields.Get(field).Label} did not read back as it was written, " +
                        $"so {Path.GetFileName(path)} was left as it is.");
            }

            if (edit.SetPicture is { } picture && written.Picture?.AsSpan().SequenceEqual(picture) != true)
                throw new MetadataFormatException(
                    $"The picture did not read back as it was written, so {Path.GetFileName(path)} was left as it is.");

            if (edit.RemovePicture && edit.SetPicture is null && written.Picture is not null)
                throw new MetadataFormatException(
                    $"The picture could not be removed from {Path.GetFileName(path)}, so it was left as it is.");

            // A removal is a promise, and the only honest way to keep one is to look afterwards.
            if ((edit.RemoveLocation || edit.RemoveAll) && written.HasLocation)
                throw new MetadataFormatException(
                    $"The location could not be removed from {Path.GetFileName(path)}, so it was left as it is.");

            if (edit.RemoveAll && written.Values.Keys.Any(k => !edit.Changes.ContainsKey(k)))
                throw new MetadataFormatException(
                    $"Not everything could be removed from {Path.GetFileName(path)}, so it was left as it is.");

            destination.Position = 0;
            if (!codec.PayloadDigest(destination).AsSpan().SequenceEqual(before))
                throw new MetadataFormatException(
                    $"Rewriting {Path.GetFileName(path)} would have changed more than its details, " +
                    "so it was left as it is.");

            // Nothing to swap, nothing to hold, and no history entry for a change that was not one.
            if (Identical(source, destination)) identical = true;
        }

        return identical ? Discard(rewrite) : SwapIn(path, rewrite, original, ct);
    }

    /// <summary>
    /// The same sequence with Windows' property handler doing the writing, for a file no codec
    /// here reads.
    /// </summary>
    /// <remarks>
    /// The handler writes in place, so it is given a <b>copy</b> to write in: until the swap the
    /// user's file has not been opened for writing by anything. The fields are read back from the
    /// copy like any other rewrite's. What cannot be done is the payload check — there is no
    /// reading of this format to hash — which is the whole difference, and why
    /// <see cref="PlannedMetadataEdit.ViaWindows"/> is carried to the pane rather than hidden.
    /// </remarks>
    private HeldVersion? EditViaWindows(PlannedMetadataEdit file, string rewrite, CancellationToken ct)
    {
        var path = file.Path;
        var name = Path.GetFileName(path);

        if (MetadataEditPlanner.Refusal(path, _probe.AttributesOf(path), viaWindows: true) is { } refusal)
            throw new MetadataFormatException(refusal.Message);

        var fallback = _fallback ?? throw new MetadataFormatException($"Nothing here can write to {name}.");
        var original = EntryStamp.Of(path) ?? throw Gone(path);

        using (var source = ReadOnlyFile.TryOpen(path) ?? throw new IOException("it could not be opened."))
        using (var destination = new FileStream(
                   rewrite, FileMode.CreateNew, FileAccess.Write, FileShare.None, bufferSize: 128 * 1024))
        {
            var buffer = new byte[128 * 1024];
            int read;
            while ((read = source.Read(buffer)) > 0)
            {
                ct.ThrowIfCancellationRequested();
                destination.Write(buffer, 0, read);
            }
        }

        fallback.Write(rewrite, file.Edit.Changes);

        var written = fallback.Read(rewrite)
                      ?? throw new MetadataFormatException($"Windows could not read {name} back after writing it, so it was left as it is.");
        foreach (var (field, value) in file.Edit.Changes)
        {
            if (!string.Equals(written.Document.Get(field), value, StringComparison.Ordinal))
                throw new MetadataFormatException(
                    $"{MetadataFields.Get(field).Label} did not read back as it was written, " +
                    $"so {name} was left as it is.");
        }

        bool identical;
        using (var source = ReadOnlyFile.TryOpen(path) ?? throw new IOException("it could not be opened."))
        using (var destination = ReadOnlyFile.TryOpen(rewrite) ?? throw new IOException("it could not be opened."))
            identical = Identical(source, destination);

        return identical ? Discard(rewrite) : SwapIn(path, rewrite, original, ct);
    }

    /// <summary>Nothing to swap, nothing to hold, and no history entry for a change that was not one.</summary>
    private static HeldVersion? Discard(string rewrite)
    {
        TryDelete(rewrite);
        return null;
    }

    private static HeldVersion SwapIn(string path, string rewrite, EntryStamp original, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        // Somebody saved over it while the rewrite was being built: theirs is newer than ours.
        if (!original.Matches(path))
            throw new MetadataFormatException(
                $"{Path.GetFileName(path)} changed while it was being edited, so it was left as it is.");

        var staged = StagedReplace.Beside(path, ReplacedMarker);
        StagedReplace.Swap(rewrite, path, staged);
        Hide(staged);

        return new HeldVersion(path, staged, EntryStamp.Of(path) ?? throw Gone(path));
    }

    /// <summary>
    /// Turns a relative change into the absolute one this file needs, so a codec only ever sets
    /// values and the read-back has something exact to compare against.
    /// </summary>
    private static MetadataEdit Resolve(MetadataEdit edit, IMetadataCodec codec, Stream source, string path)
    {
        if (edit.ShiftDateTaken is not { } shift) return edit;

        source.Position = 0;
        var taken = codec.Read(source).Get(MetadataField.DateTaken);
        if (!MetadataFields.TryParseDate(taken, out var date))
            throw new MetadataFormatException($"{Path.GetFileName(path)} has no date taken to move.");

        DateTime moved;
        try
        {
            moved = date + shift;
        }
        catch (ArgumentOutOfRangeException)
        {
            throw new MetadataFormatException($"That would move the date of {Path.GetFileName(path)} out of range.");
        }

        var changes = new Dictionary<MetadataField, string>(edit.Changes)
        {
            [MetadataField.DateTaken] = moved.ToString(MetadataFields.DateFormat, System.Globalization.CultureInfo.InvariantCulture),
        };

        return edit with { Changes = changes, ShiftDateTaken = null };
    }

    /// <summary>
    /// A held version sits beside its file, where it can be found again after a crash — and so it
    /// is hidden, or retagging an album would double the folder until the history let go.
    /// </summary>
    /// <remarks>
    /// Safe to do because the swap takes its attributes from the file being replaced, never from
    /// the one coming in: a held copy that was hidden here does not come back hidden.
    /// </remarks>
    private static void Hide(string held)
    {
        try
        {
            File.SetAttributes(held, File.GetAttributes(held) | FileAttributes.Hidden);
        }
        catch (Exception ex) when (IsEditFailure(ex)) { /* visible is untidy, not wrong */ }
    }

    /// <remarks>
    /// Uncancellable and silent on purpose, the same as every other staging move here: a cancel
    /// landing half-way through putting files back would leave some edited and some not, with no
    /// record that says which.
    /// </remarks>
    private static MetadataSwapResult Swap(IReadOnlyList<HeldVersion> versions, string asideMarker)
    {
        var swapped = new List<HeldVersion>();
        var failed = new List<FailedMetadataEdit>();

        foreach (var version in versions)
        {
            var name = Path.GetFileName(version.Path);

            if (!File.Exists(version.HeldPath))
            {
                failed.Add(new FailedMetadataEdit(version.Path, $"The other version of {name} is no longer being held."));
                continue;
            }

            // The history can reach an edit many operations later. A file changed by anything else
            // since is somebody else's work, and swapping it out would undo that instead.
            if (!version.InPlace.Matches(version.Path))
            {
                failed.Add(new FailedMetadataEdit(version.Path,
                    $"{name} has changed since, so it was left as it is. Its other version is at {version.HeldPath}."));
                continue;
            }

            var aside = StagedReplace.Beside(version.Path, asideMarker);
            try
            {
                StagedReplace.Swap(version.HeldPath, version.Path, aside);
                Hide(aside);
                swapped.Add(new HeldVersion(version.Path, aside, EntryStamp.Of(version.Path) ?? throw Gone(version.Path)));
            }
            catch (Exception ex) when (IsEditFailure(ex))
            {
                failed.Add(new FailedMetadataEdit(version.Path,
                    $"{name} could not be put back: {ex.Message} Its other version is at {version.HeldPath}.",
                    AccessDenied.Caused(ex)));
            }
        }

        return new MetadataSwapResult(swapped, failed);
    }

    private static bool Identical(Stream a, Stream b)
    {
        if (a.Length != b.Length) return false;

        a.Position = 0;
        b.Position = 0;
        var left = new byte[128 * 1024];
        var right = new byte[128 * 1024];

        while (true)
        {
            var read = a.ReadAtLeast(left, left.Length, throwOnEndOfStream: false);
            if (b.ReadAtLeast(right, read, throwOnEndOfStream: false) != read) return false;
            if (read == 0) return true;
            if (!left.AsSpan(0, read).SequenceEqual(right.AsSpan(0, read))) return false;
        }
    }

    private static IOException Gone(string path) => new($"{Path.GetFileName(path)} is no longer there.");

    private static void TryDelete(string? path)
    {
        if (path is null) return;
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception ex) when (IsEditFailure(ex)) { /* best effort */ }
    }

    private static bool IsEditFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or System.Security.SecurityException
            or NotSupportedException or ArgumentException;
}
