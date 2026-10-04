using System.Text;
using BenchmarkDotNet.Attributes;
using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>Decoding a megabyte for the text preview, in each of the encodings the ladder tells apart.</summary>
public class TextDecodeBenchmarks
{
    private const int Megabyte = 1024 * 1024;

    [Params("utf8", "utf16", "latin1", "binary")]
    public string Kind { get; set; } = "";

    private byte[] _bytes = [];

    [GlobalSetup]
    public void Setup()
    {
        var text = SampleText.Prose(Megabyte);
        _bytes = Kind switch
        {
            "utf8" => Encoding.UTF8.GetBytes(text),
            "utf16" => [.. Encoding.Unicode.GetPreamble(), .. Encoding.Unicode.GetBytes(text[..(Megabyte / 2)])],
            "latin1" => Encoding.Latin1.GetBytes(text.Replace("the", "thé")),
            _ => SampleText.Binary(Megabyte),
        };
    }

    [Benchmark]
    public TextPreview Decode() => TextPreviewReader.Decode(_bytes, moreRemains: false);
}

/// <summary>The two stream readers behind the pane: text with its line budget, and the hex dump.</summary>
public class PreviewBenchmarks
{
    private const int Megabyte = 1024 * 1024;
    private const int HexRows = 5_000;

    private byte[] _text = [];
    private byte[] _hex = [];

    [GlobalSetup]
    public void Setup()
    {
        _text = Encoding.UTF8.GetBytes(SampleText.Prose(Megabyte));
        _hex = SampleText.Binary(HexRows * 16);
    }

    [Benchmark]
    public TextPreview ReadText1Mb() => TextPreviewReader.Read(new MemoryStream(_text), _text.Length);

    [Benchmark]
    public HexPreview ReadHex5000Rows() => HexPreviewReader.Read(new MemoryStream(_hex), _hex.Length);
}

/// <summary>Syntax colouring a megabyte of source, for the two grammars with the most to do.</summary>
public class TokenizeBenchmarks
{
    private const int Megabyte = 1024 * 1024;

    [Params(SyntaxLanguage.CSharp, SyntaxLanguage.Json)]
    public SyntaxLanguage Language { get; set; }

    private string _text = "";

    [GlobalSetup]
    public void Setup() => _text = Language == SyntaxLanguage.Json ? SampleText.Json(Megabyte) : SampleText.CSharp(Megabyte);

    [Benchmark]
    public IReadOnlyList<SyntaxSpan> Tokenize1Mb() => SyntaxTokenizer.Tokenize(_text, Language);
}

/// <summary>Generated inputs, the same every run.</summary>
internal static class SampleText
{
    public static string Prose(int bytes)
    {
        const string sentence = "The index is one table, clustered by path, patched in place by the journal; a search is a range scan.\n";
        var sb = new StringBuilder(bytes + sentence.Length);
        var line = 0;
        while (sb.Length < bytes) sb.Append(line++ % 7 == 0 ? "\n" : "").Append(sentence);
        return sb.ToString(0, bytes);
    }

    public static byte[] Binary(int bytes)
    {
        var buffer = new byte[bytes];
        new Random(7).NextBytes(buffer);
        return buffer;
    }

    public static string CSharp(int bytes)
    {
        const string snippet = """
            /// <summary>Bulk-upserts one chunk of crawled entries in a single transaction.</summary>
            public void UpsertEntries(IReadOnlyList<FsEntryRow> rows, long crawlGen)
            {
                if (rows.Count == 0) return; // nothing to write
                using var conn = _db.Open();
                using var tx = conn.BeginTransaction();
                foreach (var row in rows)
                {
                    var key = row.PathKey ?? throw new InvalidOperationException("no key");
                    cmd.Parameters["@size"].Value = row.SizeBytes * 2 + 0x1F;
                    if (row.Hidden && !row.IsDirectory) count++;
                    text = $"{row.Name}: {row.ModifiedUtc:O}";
                }
                tx.Commit();
            }

            """;
        var sb = new StringBuilder(bytes + snippet.Length);
        while (sb.Length < bytes) sb.Append(snippet);
        return sb.ToString(0, bytes);
    }

    public static string Json(int bytes)
    {
        var sb = new StringBuilder(bytes + 256).Append("[\n");
        var i = 0;
        while (sb.Length < bytes)
        {
            sb.Append("  {\"id\": ").Append(i++).Append(", \"name\": \"item-").Append(i)
              .Append("\", \"size\": 12345.678, \"hidden\": false, \"tags\": [\"a\", \"b\", null], \"nested\": {\"ok\": true}},\n");
        }
        sb.Append("]\n");
        return sb.ToString(0, Math.Min(sb.Length, bytes));
    }
}
