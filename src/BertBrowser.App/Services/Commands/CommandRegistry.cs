using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.Services.Commands;

/// <summary>What one command does, and whether it can do it right now.</summary>
/// <param name="Unavailable">Why it cannot run at this moment, in words fit to show beside it, or
/// null when it can. Absent means always available.</param>
/// <param name="NeedsAPerson">It puts a dialog or another window up, or plays something. A scripted
/// run must not — it could never dismiss the one or silence the other — so the harness reports
/// these instead of running them.</param>
public sealed record CommandHandler(Action Execute, Func<string?>? Unavailable = null, bool NeedsAPerson = false);

/// <summary>
/// The other half of <see cref="CommandCatalog"/>: what each id does. A shortcut, the palette and
/// the harness all run a command through <see cref="TryExecute"/>, which asks whether it is
/// available first — so none of them can reach past a rule a menu would have enforced.
/// </summary>
public sealed class CommandRegistry
{
    private readonly Dictionary<string, CommandHandler> _handlers;
    private readonly Func<string, CommandHandler?>? _others;

    /// <param name="handlers">One per catalogue id, no more and no fewer.</param>
    /// <param name="others">Commands that are not in the catalogue because they come from data —
    /// the user's own commands, by the id the keymap binds them under.</param>
    /// <exception cref="InvalidOperationException">The catalogue and the handlers disagree. Thrown
    /// here, at startup, because the alternative is a command that is listed and does nothing —
    /// the same "an unknown id is a bug" the context menu's tags are held to.</exception>
    public CommandRegistry(
        IReadOnlyDictionary<string, CommandHandler> handlers, Func<string, CommandHandler?>? others = null)
    {
        var missing = CommandCatalog.All.Select(c => c.Id).Where(id => !handlers.ContainsKey(id)).ToList();
        var unknown = handlers.Keys.Where(id => !CommandCatalog.IsKnown(id)).ToList();

        if (missing.Count > 0 || unknown.Count > 0)
        {
            throw new InvalidOperationException(
                "The command catalogue and its handlers disagree." +
                (missing.Count > 0 ? $" No handler for: {string.Join(", ", missing)}." : "") +
                (unknown.Count > 0 ? $" Not in the catalogue: {string.Join(", ", unknown)}." : ""));
        }

        _handlers = new Dictionary<string, CommandHandler>(handlers, StringComparer.Ordinal);
        _others = others;
    }

    public CommandHandler? Find(string id) =>
        _handlers.GetValueOrDefault(id) ?? _others?.Invoke(id);

    /// <summary>Why this command cannot run right now, or null when it can.</summary>
    public string? Unavailable(string id) =>
        Find(id) is { } handler ? handler.Unavailable?.Invoke() : "There is no such command.";

    /// <summary>Runs the command if it is available. False when it is not, or does not exist.</summary>
    public bool TryExecute(string id)
    {
        if (Find(id) is not { } handler || handler.Unavailable?.Invoke() is not null) return false;
        handler.Execute();
        return true;
    }
}
