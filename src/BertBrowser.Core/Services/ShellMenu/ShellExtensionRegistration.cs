namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>A COM context-menu handler as registered under one key family
/// (<c>HKCR\*\shellex\ContextMenuHandlers\7-Zip</c>). Raw: <see cref="ShellMenuRules"/> decides
/// what it is called and whether it is shown.</summary>
/// <param name="Family">The <c>HKCR</c>-relative key it was found under.</param>
/// <param name="Name">The registration's own key name — sometimes a friendly name, sometimes the
/// CLSID again.</param>
/// <param name="ClassName">The default value of <c>HKCR\CLSID\{clsid}</c>, when there is one.</param>
public sealed record ShellHandlerRegistration(string Family, string Name, Guid Clsid, string? ClassName);

/// <summary>A static verb as registered under one key family
/// (<c>HKCR\Directory\Background\shell\git_shell</c>). Raw values; every flag is reported rather
/// than acted on here.</summary>
/// <param name="DisplayName">The <c>MUIVerb</c> value, already resolved when it was an
/// <c>@dll,-id</c> reference, else the key's default value; null when neither is set.</param>
/// <param name="Icon">The <c>Icon</c> value as written: a path, or <c>path,index</c>.</param>
/// <param name="Command">The <c>command</c> subkey's default value, or null.</param>
/// <param name="HasDelegateExecute">The command names a COM object rather than a program; the
/// command line beside it is not what runs.</param>
/// <param name="HasSubCommands">A cascading menu built from the command store.</param>
public sealed record ShellVerbRegistration(
    string Family,
    string Verb,
    string? DisplayName,
    string? Icon,
    string? Command,
    bool Extended,
    bool LegacyDisable,
    bool ProgrammaticOnly,
    bool HasDelegateExecute,
    bool HasSubCommands);
