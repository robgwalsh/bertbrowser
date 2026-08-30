using BertBrowser.App.Services;
using BertBrowser.Core.Services.ShellIntegration;

namespace BertBrowser.Harness;

/// <summary>
/// The <see cref="IFolderHandlerService"/> a scripted run gets: one that reads nothing and writes
/// nothing.
/// </summary>
/// <remarks>
/// <para>
/// Two reasons, and either alone would be enough. The real one writes the shell's <c>Directory</c>
/// and <c>Drive</c> open verbs under HKCU — machine state, outside the run's sandbox, and the one
/// setting on that page whose blast radius is <i>every folder double-click on the user's
/// machine</i>. It is the registry equivalent of starting a program on their desktop, and the same
/// answer applies.
/// </para>
/// <para>
/// And a capture must not depend on the machine it runs on: reporting
/// <see cref="FolderHandlerState.NotRegistered"/> unconditionally is what makes the settings
/// screenshot the same picture on a developer's box and on a machine where BertBrowser really does
/// own the verb.
/// </para>
/// </remarks>
internal sealed class RefusingFolderHandlerService : IFolderHandlerService
{
    public FolderHandlerState State() => FolderHandlerState.NotRegistered;

    public string? OtherProgram() => null;

    public bool TrySet(bool openFoldersHere) => false;
}
