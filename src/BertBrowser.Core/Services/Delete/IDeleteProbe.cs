namespace BertBrowser.Core.Services.Delete;

/// <summary>The filesystem questions the delete planner and executor ask about what is there.
/// Abstracted so the rules that stop a delete reaching the wrong thing can be unit-tested against
/// layouts that would otherwise need real files — including ones nobody should create for real.</summary>
public interface IDeleteProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);
}

/// <summary>Real-filesystem <see cref="IDeleteProbe"/>.</summary>
public sealed class FileSystemDeleteProbe : IDeleteProbe
{
    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);
}
