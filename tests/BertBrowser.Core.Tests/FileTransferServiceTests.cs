using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class FileTransferServiceTests : IDisposable
{
    private readonly string _root;
    private readonly FileTransferService _service = new();

    public FileTransferServiceTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-transfer-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose() => Directory.Delete(_root, recursive: true);

    private string MakeDir(params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(path);
        return path;
    }

    private string MakeFile(string content, params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    [Fact]
    public void CopyFile_IntoOtherDirectory()
    {
        var source = MakeFile("hello", "src", "a.txt");
        var dest = MakeDir("dest");

        var result = _service.CopyInto(source, dest);

        Assert.Equal(Path.Combine(dest, "a.txt"), result);
        Assert.Equal("hello", File.ReadAllText(result));
        Assert.True(File.Exists(source));
    }

    [Fact]
    public void CopyFile_Collision_GetsNumberedName()
    {
        var source = MakeFile("new", "src", "a.txt");
        var dest = MakeDir("dest");
        MakeFile("old", "dest", "a.txt");

        var result = _service.CopyInto(source, dest);

        Assert.Equal(Path.Combine(dest, "a (2).txt"), result);
        Assert.Equal("new", File.ReadAllText(result));
        Assert.Equal("old", File.ReadAllText(Path.Combine(dest, "a.txt")));
    }

    [Fact]
    public void CopyFile_IntoSameDirectory_GetsNumberedName()
    {
        var source = MakeFile("hello", "src", "a.txt");

        var result = _service.CopyInto(source, Path.Combine(_root, "src"));

        Assert.Equal(Path.Combine(_root, "src", "a (2).txt"), result);
        Assert.Equal("hello", File.ReadAllText(result));
    }

    [Fact]
    public void CopyDirectory_Recursive()
    {
        MakeFile("one", "src", "tree", "a.txt");
        MakeFile("two", "src", "tree", "sub", "b.txt");
        var dest = MakeDir("dest");

        var result = _service.CopyInto(Path.Combine(_root, "src", "tree"), dest);

        Assert.Equal("one", File.ReadAllText(Path.Combine(result, "a.txt")));
        Assert.Equal("two", File.ReadAllText(Path.Combine(result, "sub", "b.txt")));
    }

    [Fact]
    public void CopyDirectory_IntoItself_Throws()
    {
        var tree = MakeDir("tree");
        var sub = MakeDir("tree", "sub");

        Assert.Throws<InvalidOperationException>(() => _service.CopyInto(tree, tree));
        Assert.Throws<InvalidOperationException>(() => _service.CopyInto(tree, sub));
    }

    [Fact]
    public void MoveFile_IntoOtherDirectory()
    {
        var source = MakeFile("hello", "src", "a.txt");
        var dest = MakeDir("dest");

        var result = _service.MoveInto(source, dest);

        Assert.Equal(Path.Combine(dest, "a.txt"), result);
        Assert.Equal("hello", File.ReadAllText(result));
        Assert.False(File.Exists(source));
    }

    [Fact]
    public void MoveFile_IntoOwnDirectory_IsNoOp()
    {
        var source = MakeFile("hello", "src", "a.txt");

        var result = _service.MoveInto(source, Path.Combine(_root, "src"));

        Assert.Equal(source, result);
        Assert.True(File.Exists(source));
    }

    [Fact]
    public void MoveDirectory_Recursive()
    {
        MakeFile("two", "src", "tree", "sub", "b.txt");
        var dest = MakeDir("dest");

        var result = _service.MoveInto(Path.Combine(_root, "src", "tree"), dest);

        Assert.Equal(Path.Combine(dest, "tree"), result);
        Assert.Equal("two", File.ReadAllText(Path.Combine(result, "sub", "b.txt")));
        Assert.False(Directory.Exists(Path.Combine(_root, "src", "tree")));
    }

    [Fact]
    public void Move_MissingSource_Throws()
    {
        Assert.Throws<FileNotFoundException>(
            () => _service.MoveInto(Path.Combine(_root, "nope.txt"), _root));
    }
}
