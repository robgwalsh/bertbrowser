using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class GoToRulesTests
{
    private const string Home = @"C:\Users\Sam";

    private static GoToTarget? Parse(string text) => GoToRules.Parse(
        text,
        name => name.ToUpperInvariant() switch
        {
            "TEMP" => @"C:\Users\Sam\AppData\Local\Temp",
            "SYSTEMDRIVE" => "C:",
            _ => null,
        },
        Home);

    [Theory]
    [InlineData(@"C:\Windows", @"C:\Windows", @"C:\", "Windows")]
    [InlineData(@"C:\Windows\", @"C:\Windows\", @"C:\Windows\", "")]
    [InlineData(@"c:\win", @"c:\win", @"c:\", "win")]
    [InlineData(@"C:\Users\Sam\Do", @"C:\Users\Sam\Do", @"C:\Users\Sam\", "Do")]
    [InlineData(@"D:/Media/Films", @"D:\Media\Films", @"D:\Media\", "Films")]
    [InlineData(@"\\nas\share\x", @"\\nas\share\x", @"\\nas\share\", "x")]
    [InlineData("\"C:\\Program Files\"", @"C:\Program Files", @"C:\", "Program Files")]
    public void A_rooted_path_is_a_destination(string text, string path, string folder, string prefix)
    {
        Assert.Equal(new GoToTarget(path, folder, prefix), Parse(text));
    }

    [Fact]
    public void A_bare_drive_is_its_root()
    {
        // Not whatever folder the process happens to be sitting in on that drive.
        Assert.Equal(new GoToTarget(@"c:\", @"c:\", ""), Parse("c:"));
    }

    [Theory]
    [InlineData("~", Home)]
    [InlineData(@"~\Downloads", Home + @"\Downloads")]
    [InlineData("~/Downloads", Home + @"\Downloads")]
    [InlineData("%TEMP%", @"C:\Users\Sam\AppData\Local\Temp")]
    [InlineData(@"%temp%\logs", @"C:\Users\Sam\AppData\Local\Temp\logs")]
    [InlineData(@"%SystemDrive%\Windows", @"C:\Windows")]
    public void Home_and_environment_variables_are_filled_in(string text, string path)
    {
        Assert.Equal(path, Parse(text)!.Value.Path);
    }

    [Theory]
    [InlineData("")]
    [InlineData("new tab")]
    [InlineData("rename")]
    [InlineData("view:")]
    [InlineData("tabs: close")]
    [InlineData("c")]
    [InlineData("cx:")]
    [InlineData("1:")]
    [InlineData("%TEMP")]
    [InlineData("%NOSUCH%")]
    [InlineData("%%")]
    [InlineData(@"\single")]
    [InlineData("~user")]
    [InlineData("100% done")]
    public void Anything_else_is_a_search_for_a_command(string text)
    {
        Assert.Null(Parse(text));
    }

    [Fact]
    public void A_variable_that_names_something_other_than_a_path_is_not_a_destination()
    {
        Assert.Null(GoToRules.Parse("%GREETING%", _ => "hello", Home));
    }
}
