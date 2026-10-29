using BertBrowser.App.Interop;
using BertBrowser.Core.Services.ShellIntegration;

namespace BertBrowser.App.Services;

/// <summary>
/// Whether Windows opens folders and drives in BertBrowser, and the one way to change it.
/// </summary>
/// <remarks>
/// An interface for the reason <see cref="IShellNewCatalog"/> is one: this reads machine state, and
/// the harness photographs the settings page on whatever machine happens to run it. It also keeps
/// the only registry writes in the app behind something a test or a harness run can refuse.
/// </remarks>
public interface IFolderHandlerService
{
    /// <summary>Who currently owns the folder and drive open verbs.</summary>
    FolderHandlerState State();

    /// <summary>The other program holding the verb, when <see cref="State"/> says one does.</summary>
    string? OtherProgram();

    /// <summary>Takes the verbs over, or hands them back. False if the registry refused.</summary>
    bool TrySet(bool openFoldersHere);
}

/// <summary>The real thing: <see cref="FolderHandlerRegistry"/> under HKCU.</summary>
public sealed class FolderHandlerService : IFolderHandlerService
{
    public FolderHandlerState State() => FolderHandlerRegistry.State();

    public string? OtherProgram() => FolderHandlerRules.RegisteredProgram(FolderHandlerRegistry.Read());

    public bool TrySet(bool openFoldersHere) =>
        openFoldersHere ? FolderHandlerRegistry.TryRegister() : FolderHandlerRegistry.TryUnregister();
}
