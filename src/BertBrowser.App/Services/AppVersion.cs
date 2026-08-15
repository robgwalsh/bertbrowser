using System.Reflection;

namespace BertBrowser.App.Services;

/// <summary>
/// The version this build reports, for the title bar and anywhere else it is shown.
/// </summary>
/// <remarks>
/// <para>
/// Read from the assembly's informational version, which is what the release workflow stamps with
/// <c>-p:Version=</c> — the same value it hands <c>vpk pack --packVersion</c>, so what the title bar
/// says and what the installed release is called cannot drift apart. Asking Velopack instead would
/// be a second source for the same number, and one that answers nothing for a build that was never
/// installed.
/// </para>
/// <para>
/// Taken from <em>this</em> assembly rather than the entry assembly, which is not the app when the
/// UI harness is the one hosting the window.
/// </para>
/// </remarks>
public static class AppVersion
{
    /// <summary>Bare version — <c>1.4.2</c> — with no build metadata.</summary>
    public static string Display { get; } = Read();

    private static string Read()
    {
        var assembly = typeof(AppVersion).Assembly;

        var informational = assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;

        // The SDK appends "+<commit sha>" when the repository is source-linked; nobody wants that
        // in a title bar.
        if (informational is { Length: > 0 })
        {
            var plus = informational.IndexOf('+');
            return plus < 0 ? informational : informational[..plus];
        }

        return assembly.GetName().Version?.ToString(3) ?? "";
    }
}
