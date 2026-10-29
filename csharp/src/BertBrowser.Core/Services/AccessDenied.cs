using BertBrowser.Core.Interop;

namespace BertBrowser.Core.Services;

/// <summary>
/// Whether a failure was Windows refusing permission — the one bit that separates a failure an
/// administrator token could fix from every other kind.
/// </summary>
/// <remarks>
/// <para>
/// Until this existed, every executor caught <c>IOException or UnauthorizedAccessException or
/// SecurityException or NotSupportedException or ArgumentException</c> in one clause and turned it
/// into <c>ex.Message</c>. That message is localised, so nothing downstream could ever match on it:
/// "this needs a token" and "this file is open in Word" left the executor as the same thing.
/// </para>
/// <para>
/// <b>Two arms, and no more.</b> <see cref="UnauthorizedAccessException"/> is what the file APIs
/// raise for <c>ERROR_ACCESS_DENIED</c> on most paths, and what <c>FileSystemFileCopier.Translate</c>
/// manufactures for error 5. The <see cref="IOException"/> arm is there because .NET's mapping is
/// <em>not</em> uniform — it varies by primitive and by whether the path resolved to a directory —
/// and the alternative is a discriminator that works for <c>File.Delete</c> and silently does not
/// for <c>Directory.Move</c>.
/// </para>
/// <para>
/// <see cref="System.Security.SecurityException"/> is deliberately <em>excluded</em>, even though it
/// sits in every one of those catch clauses: it is a CAS-era type the modern file APIs do not throw,
/// so including it would be speculation. Every false positive here costs the user a UAC prompt that
/// cannot possibly help, which is why the rule is narrow rather than generous — and why the
/// not-same-device HRESULT, the one other Win32 code this codebase branches on, has its own theory
/// in <c>AccessDeniedTests</c> asserting it is not mistaken for this.
/// </para>
/// </remarks>
public static class AccessDenied
{
    /// <summary>The HRESULT form of <c>ERROR_ACCESS_DENIED</c>, as an <see cref="IOException"/>
    /// carries it. Spelled from the Win32 code rather than written out, the way
    /// <c>TransferExecutor.HResultNotSameDevice</c> is.</summary>
    public const int HResult = unchecked((int)(0x80070000 | (uint)CopyNative.ErrorAccessDenied));

    /// <summary>True when <paramref name="ex"/> is Windows refusing permission.</summary>
    public static bool Caused(Exception? ex) => ex switch
    {
        UnauthorizedAccessException => true,
        IOException io => io.HResult == HResult,
        _ => false,
    };
}
