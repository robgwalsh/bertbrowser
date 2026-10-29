using BertBrowser.App.Interop;
using BertBrowser.Core.Services.Metadata;

namespace BertBrowser.App.Services;

/// <summary>
/// <see cref="IPropertyFallback"/> over the Windows property system. Only the plumbing lives
/// here: which property a field is and how a value crosses are <see cref="WindowsPropertyFields"/>,
/// in Core, where they are tested.
/// </summary>
public sealed class WindowsPropertyFallback : IPropertyFallback
{
    private static readonly IReadOnlyList<string> Names = [.. WindowsPropertyFields.Canonical.Values];

    private static readonly Dictionary<string, MetadataField> ByName =
        WindowsPropertyFields.Canonical.ToDictionary(p => p.Value, p => p.Key, StringComparer.OrdinalIgnoreCase);

    public PropertyFallbackReading? Read(string path)
    {
        if (ShellProperties.ReadWritable(path, Names) is not { } properties) return null;

        var fields = new HashSet<MetadataField>();
        var values = new Dictionary<MetadataField, string>();
        foreach (var property in properties)
        {
            var field = ByName[property.Canonical];
            fields.Add(field);

            var value = WindowsPropertyFields.FromWindows(field, property.Text, property.Number, property.Utc);
            if (value.Length > 0) values[field] = value;
        }

        return new PropertyFallbackReading(new MetadataDocument(MetadataFamily.Document, values), fields);
    }

    public void Write(string path, IReadOnlyDictionary<MetadataField, string> changes)
    {
        var values = changes
            .Where(c => WindowsPropertyFields.Canonical.ContainsKey(c.Key))
            .Select(c => (WindowsPropertyFields.Canonical[c.Key], WindowsPropertyFields.ToWindows(c.Key, c.Value)))
            .ToList();

        if (ShellProperties.Write(path, values) is { } refused)
            throw new MetadataFormatException(
                $"Windows could not write to {System.IO.Path.GetFileName(path)}: {refused.TrimEnd('.')}.");
    }
}
