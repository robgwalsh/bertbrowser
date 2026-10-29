namespace BertBrowser.Core.Services;

/// <summary>
/// Expands the argument template of a custom context menu command for one target path.
/// Tokens (case-insensitive): {path} = full path, {name} = file name, {dir} = parent
/// directory. Tokens are substituted verbatim — templates should quote them
/// ("{path}") when paths may contain spaces. A blank template defaults to the
/// quoted full path.
/// </summary>
public static class CommandTemplate
{
    public static string Expand(string? template, string fullPath)
    {
        if (string.IsNullOrWhiteSpace(template))
            return Quote(fullPath);

        var directory = Path.GetDirectoryName(fullPath) ?? fullPath;
        return template
            .Replace("{path}", fullPath, StringComparison.OrdinalIgnoreCase)
            .Replace("{name}", Path.GetFileName(fullPath), StringComparison.OrdinalIgnoreCase)
            .Replace("{dir}", directory, StringComparison.OrdinalIgnoreCase);
    }

    private static string Quote(string value) => "\"" + value + "\"";
}
