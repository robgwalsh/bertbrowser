using System.IO;
using System.Windows;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Interop;

/// <summary>
/// The shell's drop-effect clipboard formats, read off and written onto a <see cref="DataObject"/>.
/// </summary>
/// <remarks>
/// <para>
/// <c>Preferred DropEffect</c> is what the <em>source</em> asks for. It is documented as a
/// clipboard-paste convention, but Explorer honours it during a drag too: dragging between two
/// folders on one volume defaults to a move, and setting this to <c>Copy</c> is what makes it copy
/// instead. That is the behaviour we want — dragging a file out of a browser into another app
/// should add a copy there, not silently take it out of the folder you were looking at — and a
/// Shift-drag still overrides it.
/// </para>
/// <para>
/// The two <c>Performed</c> formats are what the <em>target</em> writes back afterwards, and
/// together they say whether the source is expected to remove the originals. Both are frequently
/// absent — Explorer left both unset in every drop this was verified against — so
/// <see cref="DragOutContract"/> has to have a defined answer for "no report at all", and does.
/// </para>
/// </remarks>
internal static class DropEffectFormats
{
    /// <summary>What the source would like to happen. Shared with <c>FileClipboard</c>, which puts
    /// the same format on the clipboard for copy/cut/paste.</summary>
    public const string Preferred = "Preferred DropEffect";

    private const string LogicalPerformed = "Logical Performed DropEffect";
    private const string Performed = "Performed DropEffect";

    /// <summary>Marks the payload as one the source would rather see copied than moved.</summary>
    public static void SetPreferred(DataObject data, DragDropEffects effect) =>
        data.SetData(Preferred, new MemoryStream(BitConverter.GetBytes((int)effect)));

    /// <summary>The newer, more truthful of the two reports; null when the target left it unset.</summary>
    public static DropEffect? LogicalPerformedOn(DataObject data) => Read(data, LogicalPerformed);

    /// <summary>The older report, used only when the logical one is absent.</summary>
    public static DropEffect? PerformedOn(DataObject data) => Read(data, Performed);

    private static DropEffect? Read(DataObject data, string format)
    {
        try
        {
            if (!data.GetDataPresent(format)) return null;

            return data.GetData(format) switch
            {
                MemoryStream stream when stream.Length >= 4 =>
                    (DropEffect)BitConverter.ToInt32(stream.ToArray(), 0),
                int value => (DropEffect)value,
                byte[] bytes when bytes.Length >= 4 => (DropEffect)BitConverter.ToInt32(bytes, 0),
                _ => null,
            };
        }
        catch (Exception ex) when (ex is System.Runtime.InteropServices.COMException or IOException
                                      or NotSupportedException or ObjectDisposedException)
        {
            // A target that wrote something unreadable is treated as one that wrote nothing, which
            // DragOutContract already has a rule for.
            return null;
        }
    }
}
