using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.Views;

/// <summary>
/// The checklist that opens on a right-click in the column header strip: which columns are showing,
/// the curated shell properties grouped beneath, and the two commands that reach past this tab.
/// </summary>
/// <remarks>
/// <para>
/// One instance per <see cref="DirectoryTabView"/>, hung off every header that view builds. A
/// <see cref="ContextMenu"/> is an element instance and cannot be shared between views, the same
/// constraint that keeps each tab's <c>GridView</c> its own — but its <em>contents</em> are rebuilt
/// on every open rather than kept in sync, so nothing can go stale.
/// </para>
/// <para>
/// Every edit goes through <see cref="ColumnLayoutRules"/>, so this menu, a header drag and the
/// settings page cannot disagree about what adding or removing a column means.
/// </para>
/// </remarks>
internal sealed class ColumnHeaderMenu
{
    private readonly ContextMenu _menu = new();
    private readonly Func<IReadOnlyList<ColumnSetting>?> _read;
    private readonly Action<IReadOnlyList<ColumnSetting>?> _write;
    private readonly Action _saveAsDefault;
    private readonly Action _more;

    public ColumnHeaderMenu(
        Func<IReadOnlyList<ColumnSetting>?> read,
        Action<IReadOnlyList<ColumnSetting>?> write,
        Action saveAsDefault,
        Action more)
    {
        _read = read;
        _write = write;
        _saveAsDefault = saveAsDefault;
        _more = more;
        _menu.Opened += (_, _) => Rebuild();
    }

    public ContextMenu Menu => _menu;

    private void Rebuild()
    {
        _menu.Items.Clear();

        var current = ColumnLayoutRules.Normalize(_read());
        var showing = current.Select(c => c.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);

        // The built-ins first and unsorted, in catalogue order — the order they appear in is the
        // order a column set is usually built in, and shuffling it alphabetically would move
        // Name off the top.
        foreach (var spec in ColumnCatalog.BuiltIns)
        {
            if (ColumnCatalog.IsInjected(spec.Id)) continue; // Folder and Match are not choices
            _menu.Items.Add(Checkable(spec, showing));
        }

        _menu.Items.Add(new Separator());

        foreach (var group in ColumnCatalog.Curated.GroupBy(s => s.Group))
        {
            var submenu = new MenuItem { Header = group.Key };
            foreach (var spec in group)
                submenu.Items.Add(Checkable(spec, showing));
            _menu.Items.Add(submenu);
        }

        // Anything showing that neither list above named — a property added through the picker,
        // which offers the machine's whole property system. Without this its only way back off would
        // be the settings page: the picker adds and never removes, so the tick here is the one thing
        // that can turn it off again.
        var extra = current
            .Where(c => !ColumnCatalog.BuiltIns.Any(s => s.Id.Equals(c.Id, StringComparison.OrdinalIgnoreCase)))
            .Where(c => !ColumnCatalog.Curated.Any(s => s.Id.Equals(c.Id, StringComparison.OrdinalIgnoreCase)))
            .Select(c => ColumnCatalog.TryGet(c.Id))
            .OfType<ColumnSpec>()
            .ToList();

        if (extra.Count > 0)
        {
            _menu.Items.Add(new Separator());
            foreach (var spec in extra) _menu.Items.Add(Checkable(spec, showing));
        }

        _menu.Items.Add(new Separator());

        // This item is at the bottom of a menu most of a window tall, so what opens from it is
        // placed at the pointer rather than anywhere in the window — see ColumnAddPopup.
        var more = new MenuItem { Header = "More columns…" };
        more.Click += (_, _) => _more();
        _menu.Items.Add(more);

        // The two that reach past this tab. The layout is per tab — see AppSettings.FileListColumns
        // — so without this there would be no way to say "and everywhere else from now on" without
        // going to Settings for it.
        var asDefault = new MenuItem { Header = "Set as default for new tabs" };
        asDefault.Click += (_, _) => _saveAsDefault();
        _menu.Items.Add(asDefault);

        var reset = new MenuItem { Header = "Reset to default" };
        reset.Click += (_, _) => _write(null);
        _menu.Items.Add(reset);
    }

    private MenuItem Checkable(ColumnSpec spec, IReadOnlySet<string> showing)
    {
        var on = showing.Contains(spec.Id);
        var item = new MenuItem
        {
            // "__" so an underscore in a property's name renders instead of becoming an access key.
            Header = spec.Header.Replace("_", "__"),
            IsCheckable = true,
            IsChecked = on,
            Tag = spec.Id,
            // Name carries the icon and is what a row is identified by. Disabled rather than absent,
            // so the reason it cannot be turned off is visible rather than mysterious.
            IsEnabled = !string.Equals(spec.Id, ColumnCatalog.Name, StringComparison.OrdinalIgnoreCase),
        };
        item.Click += (_, _) => _write(ColumnLayoutRules.Toggle(_read(), spec.Id, !on));
        return item;
    }
}
