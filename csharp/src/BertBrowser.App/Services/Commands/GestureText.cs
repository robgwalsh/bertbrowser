using System.Text.RegularExpressions;
using BertBrowser.Core.Services.Commands;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Services.Commands;

/// <summary>
/// The shortcut a command has <em>right now</em>, for anything that prints one — a menu, a tooltip,
/// a line of status text. Shortcuts can be rebound, so "Ctrl+Z" typed into a string is a claim
/// that stops being true the first time somebody changes it; text that mentions a key asks here.
/// </summary>
/// <remarks>
/// Static, and resolved from the service graph on each call, because the callers are dialogs and
/// view models built with <c>new</c> all over the app — the route <c>ThemedWindow</c> takes to the
/// theme service for the same reason. With no graph yet (a designer, a unit of code run before
/// startup) it answers with the shipped defaults.
/// </remarks>
public static partial class GestureText
{
    private static Keymap? _defaults;

    private static Keymap Keymap =>
        App.Services?.GetService<KeymapService>()?.Current
        ?? (_defaults ??= KeymapRules.Resolve(KeymapRules.CatalogBindables(), null));

    /// <summary>The command's first shortcut, or nothing when it has none.</summary>
    public static string For(string commandId) => Keymap.GestureText(commandId);

    /// <summary>The shortcut, or — when there is none — the command's own name, so a sentence
    /// built on it still says how to do the thing: "Ctrl+Z puts these back" becomes "Undo puts
    /// these back".</summary>
    public static string OrName(string commandId) =>
        For(commandId) is { Length: > 0 } gesture
            ? gesture
            : CommandCatalog.Find(commandId)?.Name ?? commandId;

    /// <summary>
    /// Fills a template's <c>{command.id}</c> tokens. A token in brackets of its own —
    /// <c>"Refresh ({nav.refresh})"</c> — disappears with its brackets when the command has no
    /// shortcut; a bare one falls back to the command's name.
    /// </summary>
    public static string Format(string template)
    {
        var text = Bracketed().Replace(template, m =>
            For(m.Groups[1].Value) is { Length: > 0 } gesture ? $" ({gesture})" : "");
        return Bare().Replace(text, m => OrName(m.Groups[1].Value));
    }

    [GeneratedRegex(@" ?\(\{([a-z0-9.:-]+)\}\)")]
    private static partial Regex Bracketed();

    [GeneratedRegex(@"\{([a-z0-9.:-]+)\}")]
    private static partial Regex Bare();
}
