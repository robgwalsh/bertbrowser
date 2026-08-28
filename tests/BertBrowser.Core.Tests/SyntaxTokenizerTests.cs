using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class SyntaxTokenizerTests
{
    // --- picking a language ---

    [Theory]
    [InlineData("Program.cs", SyntaxLanguage.CSharp)]
    [InlineData("main.rs", SyntaxLanguage.CFamily)]
    [InlineData("app.tsx", SyntaxLanguage.JavaScript)]
    [InlineData("tsconfig.json", SyntaxLanguage.Json)]
    [InlineData("MainWindow.xaml", SyntaxLanguage.Xml)]
    [InlineData("site.scss", SyntaxLanguage.Css)]
    [InlineData("schema.SQL", SyntaxLanguage.Sql)]
    [InlineData("build.ps1", SyntaxLanguage.PowerShell)]
    [InlineData("install.sh", SyntaxLanguage.Shell)]
    [InlineData("setup.py", SyntaxLanguage.Python)]
    [InlineData("README.md", SyntaxLanguage.Markdown)]
    [InlineData("docker-compose.yml", SyntaxLanguage.Ini)]
    [InlineData("Dockerfile", SyntaxLanguage.Shell)]
    [InlineData(".gitignore", SyntaxLanguage.Ini)]
    [InlineData("notes.txt", SyntaxLanguage.None)]
    [InlineData("LICENSE", SyntaxLanguage.None)]
    public void LanguageFor_PicksTheTable(string name, SyntaxLanguage expected) =>
        Assert.Equal(expected, SyntaxTokenizer.LanguageFor(name));

    // --- the cover property, which is the one that must never break ---

    public static TheoryData<SyntaxLanguage> EveryLanguage()
    {
        var data = new TheoryData<SyntaxLanguage>();
        foreach (var language in Enum.GetValues<SyntaxLanguage>()) data.Add(language);
        return data;
    }

    /// <summary>Deliberately nasty: unterminated everything, in every order.</summary>
    private const string Adversarial =
        "\"unterminated string\nvalue = 'it\\'s' /* block\n#comment <tag attr=\"v\"> 0x1F 3.14e9\n" +
        "// line\n<!-- xml comment\n``` fence\n- bullet\n> quote\n### heading\n-- sql\n<# ps #>\n\"\"\"\n$var @attr\n";

    [Theory]
    [MemberData(nameof(EveryLanguage))]
    public void SpansCoverTheTextExactlyOnce(SyntaxLanguage language)
    {
        var spans = SyntaxTokenizer.Tokenize(Adversarial, language);
        var covered = 0;
        foreach (var span in spans)
        {
            Assert.True(span.Length > 0, "an empty span reached the view");
            Assert.Equal(covered, span.Start);
            covered += span.Length;
        }
        Assert.Equal(Adversarial.Length, covered);
    }

    [Theory]
    [MemberData(nameof(EveryLanguage))]
    public void EmptyTextYieldsNoSpans(SyntaxLanguage language) =>
        Assert.Empty(SyntaxTokenizer.Tokenize("", language));

    [Fact]
    public void AnUnknownLanguageIsOnePlainSpan()
    {
        var spans = SyntaxTokenizer.Tokenize("anything at all", SyntaxLanguage.None);
        var span = Assert.Single(spans);
        Assert.Equal(new SyntaxSpan(0, 15, SyntaxClass.Text), span);
    }

    // --- classification ---

    private static SyntaxClass ClassAt(string text, SyntaxLanguage language, int index)
    {
        foreach (var span in SyntaxTokenizer.Tokenize(text, language))
            if (index >= span.Start && index < span.Start + span.Length)
                return span.Class;
        Assert.Fail($"no span covers index {index}");
        return SyntaxClass.Text;
    }

    [Fact]
    public void AKeywordIsAKeyword() =>
        Assert.Equal(SyntaxClass.Keyword, ClassAt("public void Go()", SyntaxLanguage.CSharp, 0));

    [Fact]
    public void AnOrdinaryIdentifierIsNot() =>
        Assert.Equal(SyntaxClass.Text, ClassAt("public void Go()", SyntaxLanguage.CSharp, 12));

    [Fact]
    public void ACommentMarkerInsideAStringDoesNotStartAComment()
    {
        // The classic tokenizer bug: everything after this would go grey.
        const string code = "var url = \"http://example.com\"; var x = 1;";
        Assert.Equal(SyntaxClass.String, ClassAt(code, SyntaxLanguage.CSharp, 20));
        Assert.Equal(SyntaxClass.Keyword, ClassAt(code, SyntaxLanguage.CSharp, 32)); // the second 'var'
    }

    [Fact]
    public void AQuoteInsideACommentDoesNotStartAString()
    {
        const string code = "// it's fine\nint x = 1;";
        Assert.Equal(SyntaxClass.Comment, ClassAt(code, SyntaxLanguage.CSharp, 5));
        Assert.Equal(SyntaxClass.Keyword, ClassAt(code, SyntaxLanguage.CSharp, 13));
    }

    [Fact]
    public void AnEscapedQuoteDoesNotEndTheString()
    {
        const string code = "\"a\\\"b\" + tail";
        Assert.Equal(SyntaxClass.String, ClassAt(code, SyntaxLanguage.CSharp, 3));
        Assert.Equal(SyntaxClass.Text, ClassAt(code, SyntaxLanguage.CSharp, 9));
    }

    [Fact]
    public void AnUnterminatedBlockCommentRunsToTheEnd()
    {
        const string code = "int x;\n/* never closed\nmore text";
        Assert.Equal(SyntaxClass.Comment, ClassAt(code, SyntaxLanguage.CSharp, code.Length - 1));
    }

    [Fact]
    public void AnUnterminatedStringStopsAtTheEndOfItsLine()
    {
        // Otherwise one stray apostrophe greys out the rest of the file.
        const string code = "value = 'oops\nint x = 1;";
        Assert.Equal(SyntaxClass.Keyword, ClassAt(code, SyntaxLanguage.CSharp, 14));
    }

    [Fact]
    public void NumbersAreNumbers()
    {
        Assert.Equal(SyntaxClass.Number, ClassAt("x = 0xDEADBEEF;", SyntaxLanguage.CSharp, 6));
        Assert.Equal(SyntaxClass.Number, ClassAt("x = 3.14e9;", SyntaxLanguage.CSharp, 8));
    }

    [Fact]
    public void SqlKeywordsAreRecognisedInEitherCase()
    {
        Assert.Equal(SyntaxClass.Keyword, ClassAt("SELECT * FROM t", SyntaxLanguage.Sql, 0));
        Assert.Equal(SyntaxClass.Keyword, ClassAt("select * from t", SyntaxLanguage.Sql, 0));
    }

    [Fact]
    public void SqlUsesDoubleDashForComments() =>
        Assert.Equal(SyntaxClass.Comment, ClassAt("-- a note\nSELECT 1", SyntaxLanguage.Sql, 4));

    [Fact]
    public void XmlColoursTheElementNameAndTheAttributeValue()
    {
        const string markup = "<Button Content=\"Go\"/>";
        Assert.Equal(SyntaxClass.Punctuation, ClassAt(markup, SyntaxLanguage.Xml, 0));
        Assert.Equal(SyntaxClass.Keyword, ClassAt(markup, SyntaxLanguage.Xml, 3));
        Assert.Equal(SyntaxClass.Text, ClassAt(markup, SyntaxLanguage.Xml, 9));
        Assert.Equal(SyntaxClass.String, ClassAt(markup, SyntaxLanguage.Xml, 17));
    }

    [Fact]
    public void XmlCommentsAreComments() =>
        Assert.Equal(SyntaxClass.Comment, ClassAt("<!-- hidden --><b/>", SyntaxLanguage.Xml, 6));

    [Fact]
    public void MarkdownHeadingsAndFencesAreDistinguished()
    {
        const string doc = "# Title\ntext\n```\ncode\n```\n";
        Assert.Equal(SyntaxClass.Keyword, ClassAt(doc, SyntaxLanguage.Markdown, 2));
        Assert.Equal(SyntaxClass.Text, ClassAt(doc, SyntaxLanguage.Markdown, 9));
        Assert.Equal(SyntaxClass.String, ClassAt(doc, SyntaxLanguage.Markdown, 18)); // inside the fence
    }

    [Fact]
    public void PowerShellUsesItsOwnBlockComment() =>
        Assert.Equal(SyntaxClass.Comment, ClassAt("<# note #>\nif ($x) {}", SyntaxLanguage.PowerShell, 4));

    [Fact]
    public void JsonColoursItsLiterals()
    {
        const string json = "{ \"on\": true }";
        Assert.Equal(SyntaxClass.String, ClassAt(json, SyntaxLanguage.Json, 3));
        Assert.Equal(SyntaxClass.Keyword, ClassAt(json, SyntaxLanguage.Json, 9));
    }

    [Fact]
    public void AdjacentRunsOfTheSameClassAreMerged()
    {
        // Neither word is a keyword, and a space is neither punctuation nor part of an
        // identifier — so the line arrives as one span rather than three.
        var spans = SyntaxTokenizer.Tokenize("abc def", SyntaxLanguage.CSharp);
        Assert.Equal(new SyntaxSpan(0, 7, SyntaxClass.Text), Assert.Single(spans));
    }
}
