using BenchmarkDotNet.Running;

namespace BertBrowser.Bench.Infrastructure;

/// <summary>
/// The stable id a Tier A result is filed under: <c>core.&lt;class&gt;.&lt;method&gt;(&lt;param&gt;=&lt;value&gt;,…)</c>.
/// </summary>
/// <remarks>
/// Derived from names, not from BenchmarkDotNet's own display name, so a BDN upgrade that changes how
/// it formats a case cannot orphan every baseline. Renaming a benchmark class or method does, and is
/// meant to: a renamed benchmark is a new one.
/// </remarks>
internal static class BenchId
{
    private const string ClassSuffix = "Benchmarks";

    public static string For(BenchmarkCase benchmarkCase)
    {
        var type = benchmarkCase.Descriptor.Type.Name;
        if (type.EndsWith(ClassSuffix, StringComparison.Ordinal)) type = type[..^ClassSuffix.Length];

        var id = $"core.{type.ToLowerInvariant()}.{benchmarkCase.Descriptor.WorkloadMethod.Name.ToLowerInvariant()}";

        var parameters = benchmarkCase.Parameters.Items;
        if (parameters.Count > 0)
        {
            id += "(" + string.Join(",", parameters.Select(p => $"{p.Name.ToLowerInvariant()}={p.Value}")) + ")";
        }

        return id;
    }

    public static IReadOnlyDictionary<string, string> Parameters(BenchmarkCase benchmarkCase, int scale)
    {
        var dict = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var p in benchmarkCase.Parameters.Items)
            dict[p.Name] = p.Value?.ToString() ?? "null";
        dict["scale"] = scale.ToString(System.Globalization.CultureInfo.InvariantCulture);
        return dict;
    }
}
