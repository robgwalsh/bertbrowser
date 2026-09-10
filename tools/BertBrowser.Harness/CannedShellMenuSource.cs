using BertBrowser.App.Services;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.Harness;

/// <summary>
/// The <see cref="IShellMenuSource"/> a scripted run gets: a fixed set of extensions and no COM.
/// </summary>
/// <remarks>
/// <para>
/// The real source loads whatever context-menu handlers this machine has into the process and,
/// when an entry is picked, runs them — 7-Zip would put a dialog on the desktop, which is the
/// interruption this harness exists to avoid, and a screenshot would depend on what the developer
/// happens to have installed. This one offers what a typical developer's machine offers, so the
/// menu section and the Settings checklist can be photographed the same way everywhere.
/// </para>
/// <para>
/// It is honest about the two things the real one decides from settings: the master switch and
/// the hidden list both apply, through the same <see cref="ShellMenuRules.Visible"/>, so a script
/// can untick an extension and see it go.
/// </para>
/// </remarks>
internal sealed class CannedShellMenuSource : IShellMenuSource
{
    private static readonly Guid SevenZip = new("23170F69-40C1-278A-1000-000100020000");
    private static readonly Guid TortoiseSvn = new("30351346-7B7D-4FCC-81B4-1E394CA267EB");

    private readonly AppSettings _settings;

    public CannedShellMenuSource(AppSettings settings)
    {
        _settings = settings;
    }

    /// <summary>Every entry a script picked, by header, so a run can prove a click reached here.</summary>
    public List<string> Invocations { get; } = [];

    private static readonly IReadOnlyList<ShellExtension> Catalog =
    [
        new ShellExtension(
            ShellExtension.VerbId("git_shell"), "Open Git Bash here", ShellExtensionKind.Verb, null,
            [
                new ShellVerbRegistration(
                    ShellMenuKeys.Directory, "git_shell", "Open Git Bash here", null,
                    "\"C:\\Program Files\\Git\\git-bash.exe\" \"--cd=%1\"", false, false, false, false, false),
                new ShellVerbRegistration(
                    ShellMenuKeys.Background, "git_shell", "Open Git Bash here", null,
                    "\"C:\\Program Files\\Git\\git-bash.exe\" \"--cd=%v.\"", false, false, false, false, false),
            ],
            [ShellMenuKeys.Directory, ShellMenuKeys.Background]),
        new ShellExtension(
            ShellExtension.VerbId("VSCode"), "Open with Code", ShellExtensionKind.Verb, null,
            [
                new ShellVerbRegistration(
                    ShellMenuKeys.Star, "VSCode", "Open with Code", null,
                    "\"C:\\Program Files\\Microsoft VS Code\\Code.exe\" \"%1\"", false, false, false, false, false),
            ],
            [ShellMenuKeys.Star, ShellMenuKeys.Directory, ShellMenuKeys.Background]),
        new ShellExtension(
            ShellExtension.HandlerId(SevenZip), "7-Zip", ShellExtensionKind.Handler, SevenZip, [],
            [ShellMenuKeys.Star, ShellMenuKeys.Directory, ShellMenuKeys.Folder, ShellMenuKeys.Drive]),
        new ShellExtension(
            ShellExtension.HandlerId(TortoiseSvn), "TortoiseSVN", ShellExtensionKind.Handler, TortoiseSvn, [],
            [ShellMenuKeys.Star, ShellMenuKeys.Directory, ShellMenuKeys.Background]),
    ];

    public ShellMenuSession? Open(IReadOnlyList<ShellMenuTarget> targets, ShellMenuContext context, string folder)
    {
        var visible = ShellMenuRules.Visible(Catalog, _settings.ShowShellExtensions, _settings.HiddenShellExtensions);

        var entries = new List<ShellMenuEntry>();
        foreach (var extension in visible)
        {
            switch (extension.Name)
            {
                case "Open Git Bash here":
                    entries.Add(ShellMenuEntry.Item("Open Git Bash here", extension));
                    break;
                case "Open with Code":
                    entries.Add(ShellMenuEntry.Item("Open with Code", extension));
                    break;
                case "7-Zip" when context == ShellMenuContext.Items:
                    entries.Add(ShellMenuEntry.Submenu("7-Zip",
                    [
                        ShellMenuEntry.Item("Add to archive...", extension),
                        ShellMenuEntry.Item("Extract Here", extension, enabled: targets.Any(t => !t.IsDirectory)),
                        ShellMenuEntry.Separator,
                        ShellMenuEntry.Item("Test archive", extension, enabled: false),
                    ]));
                    break;
                case "TortoiseSVN":
                    entries.Add(ShellMenuEntry.Submenu("TortoiseSVN",
                    [
                        ShellMenuEntry.Item("SVN Checkout...", extension),
                        ShellMenuEntry.Separator,
                        ShellMenuEntry.Item("Settings", extension),
                    ]));
                    break;
            }
        }

        var tidy = ShellMenuRules.Tidy(entries);
        if (tidy.Count == 0) return null;

        return new ShellMenuSession(
            tidy,
            (entry, _) =>
            {
                Invocations.Add(entry.Header);
                return $"The harness does not run shell extensions: '{entry.Header}' was not invoked.";
            },
            () => { });
    }

    public Task<IReadOnlyList<ShellExtension>> CatalogAsync() => Task.FromResult(Catalog);
}
