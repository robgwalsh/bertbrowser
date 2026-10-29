using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class CommandTemplateTests
{
    private const string SamplePath = @"C:\data\docs\report.txt";

    [Fact]
    public void BlankTemplate_DefaultsToQuotedPath() =>
        Assert.Equal("\"C:\\data\\docs\\report.txt\"", CommandTemplate.Expand("", SamplePath));

    [Fact]
    public void NullTemplate_DefaultsToQuotedPath() =>
        Assert.Equal("\"C:\\data\\docs\\report.txt\"", CommandTemplate.Expand(null, SamplePath));

    [Fact]
    public void ExpandsAllTokens() =>
        Assert.Equal(
            @"""C:\data\docs\report.txt"" report.txt C:\data\docs",
            CommandTemplate.Expand("\"{path}\" {name} {dir}", SamplePath));

    [Fact]
    public void TokensAreCaseInsensitive() =>
        Assert.Equal(SamplePath, CommandTemplate.Expand("{PATH}", SamplePath));

    [Fact]
    public void TemplateWithoutTokens_IsReturnedVerbatim() =>
        Assert.Equal("--verbose", CommandTemplate.Expand("--verbose", SamplePath));
}
