using System.Xml.Linq;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The sign-in task definition. Several of these elements are silent defaults that would break the
/// feature rather than fail loudly, so the tests are the specification.
/// </summary>
public class IndexerAutoStartTaskTests
{
    private const string Sid = "S-1-5-21-1845729561-1936007304-1872547691-1001";
    private const string Helper = @"C:\Users\Rob\AppData\Local\BertBrowser\current\BertBrowser.Indexer.exe";
    private const string DataDir = @"C:\Users\Rob\.bertbrowser";

    private static XDocument Parse(string helper = Helper, string dataDir = DataDir, string sid = Sid) =>
        XDocument.Parse(IndexerAutoStartTask.BuildXml(helper, dataDir, sid));

    private static XNamespace Ns => "http://schemas.microsoft.com/windows/2004/02/mit/task";

    private static string Value(XDocument doc, string element) =>
        doc.Descendants(Ns + element).First().Value;

    /// <summary>
    /// <b>The whole point of the task.</b> Without HighestAvailable it runs unelevated, cannot read
    /// the MFT, and the prompt it was meant to abolish comes back.
    /// </summary>
    [Fact]
    public void RunsElevated()
    {
        Assert.Equal("HighestAvailable", Value(Parse(), "RunLevel"));
    }

    /// <summary>
    /// As the signed-in user, not SYSTEM. The helper derives its pipe name from its own token, so a
    /// SYSTEM helper would listen on a name no app ever looks for.
    /// </summary>
    [Fact]
    public void RunsAsTheUserWhoseIndexItIs()
    {
        var doc = Parse();

        Assert.Equal("InteractiveToken", Value(doc, "LogonType"));
        Assert.All(doc.Descendants(Ns + "UserId"), id => Assert.Equal(Sid, id.Value));
    }

    [Fact]
    public void StartsAtSignIn()
    {
        Assert.Single(Parse().Descendants(Ns + "LogonTrigger"));
    }

    /// <summary>
    /// <b>The default is three days, and it would kill the helper.</b> PT0S means no limit, which is
    /// what a process meant to run until sign-out needs.
    /// </summary>
    [Fact]
    public void NeverTimesTheHelperOut()
    {
        Assert.Equal("PT0S", Value(Parse(), "ExecutionTimeLimit"));
    }

    /// <summary>
    /// The helper refuses to run twice anyway, but a task that queued a second one would leave the
    /// scheduler reporting failures the user cannot explain.
    /// </summary>
    [Fact]
    public void DoesNotStartASecondHelper()
    {
        Assert.Equal("IgnoreNew", Value(Parse(), "MultipleInstancesPolicy"));
    }

    /// <summary>
    /// A laptop is exactly where the index matters most, and every one of these defaults would stop
    /// or refuse to start the helper on battery or when the machine goes idle.
    /// </summary>
    [Fact]
    public void KeepsRunningOnBatteryAndWhenIdle()
    {
        var doc = Parse();

        Assert.Equal("false", Value(doc, "DisallowStartIfOnBatteries"));
        Assert.Equal("false", Value(doc, "StopIfGoingOnBatteries"));
        Assert.Equal("false", Value(doc, "StopOnIdleEnd"));
        Assert.Equal("false", Value(doc, "RunOnlyIfIdle"));
    }

    [Fact]
    public void PassesOnlyTheDataDirectory()
    {
        var arguments = Value(Parse(), "Arguments");

        Assert.Equal($"--data-dir \"{DataDir}\"", arguments);
        Assert.DoesNotContain("--pipe", arguments, StringComparison.Ordinal);
        Assert.DoesNotContain("--parent-pid", arguments, StringComparison.Ordinal);
    }

    [Fact]
    public void PointsAtTheHelper()
    {
        Assert.Equal(Helper, Value(Parse(), "Command"));
    }

    /// <summary>
    /// A profile path may perfectly well contain an ampersand, and an unescaped one makes the whole
    /// definition unparseable — which the scheduler reports as a failure with no clue in it.
    /// </summary>
    [Fact]
    public void EscapesPathsTheXmlWouldOtherwiseChokeOn()
    {
        var doc = Parse(dataDir: @"C:\Users\Bill & Ted\.bertbrowser");

        Assert.Contains("Bill & Ted", Value(doc, "Arguments"), StringComparison.Ordinal);
    }

    /// <summary>
    /// An update moves the install directory, leaving the task aimed at an executable that is gone.
    /// It cannot be repaired without another prompt, so the settings page has to be able to say so.
    /// </summary>
    [Theory]
    [InlineData(@"C:\Old\BertBrowser.Indexer.exe", @"C:\New\BertBrowser.Indexer.exe", true)]
    [InlineData(@"C:\App\BertBrowser.Indexer.exe", @"C:\App\BertBrowser.Indexer.exe", false)]
    [InlineData(@"c:\app\bertbrowser.indexer.exe", @"C:\App\BertBrowser.Indexer.exe", false)]
    [InlineData("\"C:\\App\\BertBrowser.Indexer.exe\"", @"C:\App\BertBrowser.Indexer.exe", false)]
    public void KnowsWhenARegisteredTaskPointsSomewhereElse(string registered, string current, bool expected)
    {
        Assert.Equal(expected, IndexerAutoStartTask.NeedsReregistration(registered, current));
    }

    [Fact]
    public void SaysNothingIsWrongWhenThereIsNoTask()
    {
        Assert.False(IndexerAutoStartTask.NeedsReregistration(null, Helper));
        Assert.False(IndexerAutoStartTask.NeedsReregistration("", Helper));
    }
}
