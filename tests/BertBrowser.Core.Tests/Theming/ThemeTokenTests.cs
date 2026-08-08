using System.Reflection;
using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

public class ThemeTokenTests
{
    private static IEnumerable<string> DeclaredConstants() => typeof(ThemeToken)
        .GetFields(BindingFlags.Public | BindingFlags.Static | BindingFlags.FlattenHierarchy)
        .Where(f => f is { IsLiteral: true, IsInitOnly: false } && f.FieldType == typeof(string))
        .Select(f => (string)f.GetRawConstantValue()!);

    /// <summary>
    /// The point of the whole design: a colour cannot be themed without also being describable in the
    /// editor. Add a <c>const</c> to <see cref="ThemeToken"/> and forget its descriptor and this
    /// fails — which is the only compile-time-ish safety net the App project's XAML gets.
    /// </summary>
    [Fact]
    public void Every_declared_constant_has_a_descriptor()
    {
        var missing = DeclaredConstants().Where(key => !ThemeToken.IsKnown(key)).ToList();
        Assert.Empty(missing);
    }

    [Fact]
    public void Every_descriptor_corresponds_to_a_declared_constant()
    {
        var declared = DeclaredConstants().ToHashSet(StringComparer.Ordinal);
        var orphans = ThemeToken.All.Where(key => !declared.Contains(key)).ToList();
        Assert.Empty(orphans);
    }

    [Fact]
    public void Token_keys_are_unique()
    {
        var duplicates = ThemeToken.All.GroupBy(k => k, StringComparer.Ordinal)
            .Where(g => g.Count() > 1)
            .Select(g => g.Key)
            .ToList();
        Assert.Empty(duplicates);
    }

    [Fact]
    public void Every_descriptor_is_presentable()
    {
        foreach (var descriptor in ThemeToken.Descriptors)
        {
            Assert.False(string.IsNullOrWhiteSpace(descriptor.Group), $"{descriptor.Key} has no group");
            Assert.False(string.IsNullOrWhiteSpace(descriptor.DisplayName), $"{descriptor.Key} has no display name");
        }
    }

    /// <summary>Keys double as WPF resource keys, so they must look like the ones in XAML.</summary>
    [Fact]
    public void Token_keys_are_dotted_Theme_paths()
    {
        foreach (var key in ThemeToken.All)
        {
            Assert.StartsWith("Theme.", key, StringComparison.Ordinal);
            Assert.Equal(3, key.Split('.').Length);
        }
    }

    [Fact]
    public void The_editors_default_list_is_a_useful_size()
    {
        var core = ThemeToken.Descriptors.Count(d => d.IsCore);
        Assert.InRange(core, 8, 24);
    }

    [Fact]
    public void Groups_are_contiguous_so_the_editor_can_list_them_in_order()
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        string? previous = null;

        foreach (var descriptor in ThemeToken.Descriptors)
        {
            if (descriptor.Group == previous) continue;
            Assert.True(seen.Add(descriptor.Group), $"group '{descriptor.Group}' is split across the list");
            previous = descriptor.Group;
        }
    }
}
