namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>
/// One line of the shell's contribution to a menu, in the form the view renders: text ready for
/// a WPF header, its children, and an opaque handle the App's session turns back into an
/// invocation.
/// </summary>
/// <remarks>
/// <see cref="Command"/> and <see cref="Icon"/> are <c>object</c> so this project holds no COM or
/// WPF type: the App wraps a handler and a command id in the first and an <c>ImageSource</c> in the
/// second, and the harness puts whatever it likes in both.
/// </remarks>
public sealed record ShellMenuEntry(
    string Header,
    bool IsSeparator,
    bool IsEnabled,
    IReadOnlyList<ShellMenuEntry> Children,
    object? Command,
    object? Icon = null)
{
    public static ShellMenuEntry Separator { get; } = new("", IsSeparator: true, IsEnabled: false, [], null);

    public static ShellMenuEntry Item(string header, object? command, bool enabled = true, object? icon = null) =>
        new(header, IsSeparator: false, enabled, [], command, icon);

    public static ShellMenuEntry Submenu(string header, IReadOnlyList<ShellMenuEntry> children, object? icon = null) =>
        new(header, IsSeparator: false, IsEnabled: true, children, null, icon);

    public bool HasChildren => Children.Count > 0;
}
