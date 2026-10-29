using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Services.Commands;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// XAML's way of saying which command a control is for, so the shortcut it shows is the one in
/// force rather than one typed beside it.
/// </summary>
/// <remarks>
/// <list type="bullet">
/// <item><c>v:Cmd.Id="file.new-folder"</c> on a <see cref="MenuItem"/> fills its gesture text.</item>
/// <item><c>v:Cmd.Tip="Refresh ({nav.refresh})"</c> sets a tooltip from a template.</item>
/// <item><c>v:Cmd.Text="… ({tools.changes}) …"</c> does the same for a <see cref="TextBlock"/>.</item>
/// </list>
/// The template rules are <see cref="GestureText.Format"/>'s. Every element given one of these is
/// remembered weakly and rewritten when the keymap changes, so a rebind shows up without a restart.
/// </remarks>
public static class Cmd
{
    public static readonly DependencyProperty IdProperty = DependencyProperty.RegisterAttached(
        "Id", typeof(string), typeof(Cmd), new PropertyMetadata(null, OnChanged));

    public static readonly DependencyProperty TipProperty = DependencyProperty.RegisterAttached(
        "Tip", typeof(string), typeof(Cmd), new PropertyMetadata(null, OnChanged));

    public static readonly DependencyProperty TextProperty = DependencyProperty.RegisterAttached(
        "Text", typeof(string), typeof(Cmd), new PropertyMetadata(null, OnChanged));

    private static readonly List<WeakReference<DependencyObject>> Tracked = [];
    private static KeymapService? _listeningTo;

    public static string? GetId(DependencyObject element) => (string?)element.GetValue(IdProperty);

    public static void SetId(DependencyObject element, string? value) => element.SetValue(IdProperty, value);

    public static string? GetTip(DependencyObject element) => (string?)element.GetValue(TipProperty);

    public static void SetTip(DependencyObject element, string? value) => element.SetValue(TipProperty, value);

    public static string? GetText(DependencyObject element) => (string?)element.GetValue(TextProperty);

    public static void SetText(DependencyObject element, string? value) => element.SetValue(TextProperty, value);

    private static void OnChanged(DependencyObject element, DependencyPropertyChangedEventArgs e)
    {
        Listen();
        // Tabs come and go for the life of the window, so the dead are swept as the list grows
        // rather than only when the keymap next changes — which may be never.
        if (Tracked.Count > 0 && Tracked.Count % 64 == 0)
            Tracked.RemoveAll(w => !w.TryGetTarget(out _));
        if (!Tracked.Exists(w => w.TryGetTarget(out var known) && ReferenceEquals(known, element)))
            Tracked.Add(new WeakReference<DependencyObject>(element));
        Apply(element);
    }

    private static void Apply(DependencyObject element)
    {
        if (GetId(element) is { } id && element is MenuItem item)
            item.InputGestureText = GestureText.For(id);
        if (GetTip(element) is { } tip && element is FrameworkElement framework)
            framework.ToolTip = GestureText.Format(tip);
        if (GetText(element) is { } text && element is TextBlock block)
            block.Text = GestureText.Format(text);
    }

    /// <summary>Hooks the keymap once it exists. Until then nothing can change, so there is
    /// nothing to hear.</summary>
    private static void Listen()
    {
        if (App.Services?.GetService<KeymapService>() is not { } keymap || ReferenceEquals(keymap, _listeningTo))
            return;

        if (_listeningTo is not null) _listeningTo.Changed -= OnKeymapChanged;
        _listeningTo = keymap;
        keymap.Changed += OnKeymapChanged;
    }

    private static void OnKeymapChanged(object? sender, EventArgs e)
    {
        for (var i = Tracked.Count - 1; i >= 0; i--)
        {
            if (Tracked[i].TryGetTarget(out var element)) Apply(element);
            else Tracked.RemoveAt(i);
        }
    }
}
