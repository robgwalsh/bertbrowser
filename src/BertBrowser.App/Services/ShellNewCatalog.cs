using BertBrowser.App.Interop;
using BertBrowser.Core.Services.NewItem;

namespace BertBrowser.App.Services;

/// <summary>What new-file types Windows knows about.</summary>
/// <remarks>An interface so the harness can hand back a fixed list rather than photographing
/// whatever happens to be installed on the machine the script runs on.</remarks>
public interface IShellNewCatalog
{
    /// <summary>Every type worth offering, off the UI thread — HKEY_CLASSES_ROOT has thousands of
    /// subkeys and this walks all of them.</summary>
    Task<IReadOnlyList<NewFileTemplate>> ReadAsync();
}

/// <summary>Reads Windows' own ShellNew registration. Read-only; see
/// <see cref="ShellNewRegistry"/>.</summary>
public sealed class ShellNewCatalog : IShellNewCatalog
{
    public Task<IReadOnlyList<NewFileTemplate>> ReadAsync() => Task.Run(() =>
        ShellNewImport.ToTemplates(
            ShellNewRegistry.Read(),
            ShellNewRegistry.LoadIndirectString,
            File.Exists,
            ShellNewRegistry.TemplateRoots(),
            SaveTemplateData));

    /// <summary>Writes a registry-held template out to a real file, once, so everything downstream
    /// sees one shape: a template is either empty or a file on disk.</summary>
    private static string? SaveTemplateData(string extension, byte[] data)
    {
        try
        {
            Directory.CreateDirectory(AppPaths.TemplatesDir);
            var path = Path.Combine(
                AppPaths.TemplatesDir, $"shellnew-{extension.TrimStart('.').ToLowerInvariant()}");
            File.WriteAllBytes(path, data);
            return path;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                       or ArgumentException or NotSupportedException)
        {
            // One type fewer on the menu, not a failed import.
            return null;
        }
    }
}
