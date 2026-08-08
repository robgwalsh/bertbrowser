namespace BertBrowser.Core.Theming;

/// <summary>
/// Every colour the app can be themed with. These strings are used verbatim as WPF resource keys
/// (<c>{StaticResource Theme.List.HoverBackground}</c>), as JSON property names in a theme file, and
/// as the identity of a row in the theme editor.
/// </summary>
/// <remarks>
/// <see cref="Descriptors"/> is the single source of truth; <see cref="All"/> is derived from it, so
/// a token cannot be themed without also being describable in the editor. <c>ThemeTokenTests</c>
/// reflects over the constants below and fails if one is missing a descriptor.
/// </remarks>
public static class ThemeToken
{
    // Surfaces
    public const string WindowBackground = "Theme.Window.Background";
    public const string SurfaceBackground = "Theme.Surface.Background";
    public const string SurfaceRaised = "Theme.Surface.Raised";
    public const string OverlayBackground = "Theme.Overlay.Background";
    public const string OverlayBorder = "Theme.Overlay.Border";
    public const string ShadowColor = "Theme.Shadow.Color";

    // Text
    public const string TextPrimary = "Theme.Text.Primary";
    public const string TextSecondary = "Theme.Text.Secondary";
    public const string TextMuted = "Theme.Text.Muted";
    public const string TextDisabled = "Theme.Text.Disabled";
    public const string TextOnAccent = "Theme.Text.OnAccent";
    public const string TextLink = "Theme.Text.Link";
    public const string TextPlaceholder = "Theme.Text.Placeholder";

    // Borders
    public const string BorderDefault = "Theme.Border.Default";
    public const string BorderSubtle = "Theme.Border.Subtle";
    public const string BorderFocus = "Theme.Border.Focus";
    public const string BorderActive = "Theme.Border.Active";

    // Accent
    public const string AccentBackground = "Theme.Accent.Background";
    public const string AccentHoverBackground = "Theme.Accent.HoverBackground";
    public const string AccentPressedBackground = "Theme.Accent.PressedBackground";
    public const string AccentForeground = "Theme.Accent.Foreground";
    public const string AccentMuted = "Theme.Accent.Muted";

    // Lists (file list, folder tree, bookmarks, thumbnails)
    public const string ListBackground = "Theme.List.Background";
    public const string ListHoverBackground = "Theme.List.HoverBackground";
    public const string ListSelectedBackground = "Theme.List.SelectedBackground";
    public const string ListSelectedForeground = "Theme.List.SelectedForeground";
    public const string ListSelectedHoverBackground = "Theme.List.SelectedHoverBackground";
    public const string ListSelectedInactiveBackground = "Theme.List.SelectedInactiveBackground";
    public const string ListSelectedInactiveForeground = "Theme.List.SelectedInactiveForeground";
    public const string ListDropBackground = "Theme.List.DropBackground";
    public const string ListDropBorder = "Theme.List.DropBorder";
    public const string ListHeaderBackground = "Theme.List.HeaderBackground";
    public const string ListHeaderForeground = "Theme.List.HeaderForeground";
    public const string ListHeaderHoverBackground = "Theme.List.HeaderHoverBackground";
    public const string ListHeaderBorder = "Theme.List.HeaderBorder";

    // Folder tree
    public const string TreeChevronForeground = "Theme.Tree.ChevronForeground";
    public const string TreeChevronHoverForeground = "Theme.Tree.ChevronHoverForeground";

    // Tab strip
    public const string TabStripBackground = "Theme.Tab.StripBackground";
    public const string TabActiveBackground = "Theme.Tab.ActiveBackground";
    public const string TabActiveForeground = "Theme.Tab.ActiveForeground";
    public const string TabInactiveBackground = "Theme.Tab.InactiveBackground";
    public const string TabInactiveForeground = "Theme.Tab.InactiveForeground";
    public const string TabHoverBackground = "Theme.Tab.HoverBackground";
    public const string TabBorder = "Theme.Tab.Border";
    public const string TabActiveIndicator = "Theme.Tab.ActiveIndicator";

    // Title bar
    public const string TitleBarBackground = "Theme.TitleBar.Background";
    public const string TitleBarInactiveBackground = "Theme.TitleBar.InactiveBackground";
    public const string TitleBarForeground = "Theme.TitleBar.Foreground";
    public const string TitleBarInactiveForeground = "Theme.TitleBar.InactiveForeground";
    public const string TitleBarButtonHoverBackground = "Theme.TitleBar.ButtonHoverBackground";
    public const string TitleBarButtonPressedBackground = "Theme.TitleBar.ButtonPressedBackground";
    public const string TitleBarCloseHoverBackground = "Theme.TitleBar.CloseHoverBackground";
    public const string TitleBarCloseHoverForeground = "Theme.TitleBar.CloseHoverForeground";
    public const string TitleBarBorder = "Theme.TitleBar.Border";

    // Toolbar
    public const string ToolbarBackground = "Theme.Toolbar.Background";
    public const string ToolbarForeground = "Theme.Toolbar.Foreground";
    public const string ToolbarHoverBackground = "Theme.Toolbar.HoverBackground";
    public const string ToolbarPressedBackground = "Theme.Toolbar.PressedBackground";
    public const string ToolbarCheckedForeground = "Theme.Toolbar.CheckedForeground";

    // Text inputs
    public const string InputBackground = "Theme.Input.Background";
    public const string InputForeground = "Theme.Input.Foreground";
    public const string InputBorder = "Theme.Input.Border";
    public const string InputSelectionBackground = "Theme.Input.SelectionBackground";
    public const string InputSelectionForeground = "Theme.Input.SelectionForeground";
    public const string InputCaret = "Theme.Input.Caret";

    // Buttons (primary buttons reuse the accent tokens)
    public const string ButtonSecondaryBackground = "Theme.Button.SecondaryBackground";
    public const string ButtonSecondaryHoverBackground = "Theme.Button.SecondaryHoverBackground";
    public const string ButtonSecondaryPressedBackground = "Theme.Button.SecondaryPressedBackground";
    public const string ButtonSecondaryForeground = "Theme.Button.SecondaryForeground";
    public const string ButtonDisabledBackground = "Theme.Button.DisabledBackground";
    public const string ButtonDisabledForeground = "Theme.Button.DisabledForeground";

    // Context menus
    public const string MenuBackground = "Theme.Menu.Background";
    public const string MenuForeground = "Theme.Menu.Foreground";
    public const string MenuBorder = "Theme.Menu.Border";
    public const string MenuHoverBackground = "Theme.Menu.HoverBackground";
    public const string MenuHoverForeground = "Theme.Menu.HoverForeground";
    public const string MenuSeparator = "Theme.Menu.Separator";
    public const string MenuIconForeground = "Theme.Menu.IconForeground";
    public const string MenuGestureForeground = "Theme.Menu.GestureForeground";
    public const string MenuDisabledForeground = "Theme.Menu.DisabledForeground";

    // Scrollbars
    public const string ScrollBarBackground = "Theme.ScrollBar.Background";
    public const string ScrollBarThumb = "Theme.ScrollBar.Thumb";
    public const string ScrollBarThumbHover = "Theme.ScrollBar.ThumbHover";
    public const string ScrollBarThumbActive = "Theme.ScrollBar.ThumbActive";

    // Status bar
    public const string StatusBarBackground = "Theme.StatusBar.Background";
    public const string StatusBarForeground = "Theme.StatusBar.Foreground";
    public const string StatusBarMutedForeground = "Theme.StatusBar.MutedForeground";
    public const string StatusBarHoverBackground = "Theme.StatusBar.HoverBackground";

    // Feedback
    public const string ErrorForeground = "Theme.Error.Foreground";
    public const string ErrorBackground = "Theme.Error.Background";
    public const string ErrorBorder = "Theme.Error.Border";
    public const string WarningForeground = "Theme.Warning.Foreground";
    public const string WarningBackground = "Theme.Warning.Background";
    public const string WarningBorder = "Theme.Warning.Border";
    public const string ProgressIndicator = "Theme.Progress.Indicator";
    public const string ProgressTrack = "Theme.Progress.Track";

    // Small controls
    public const string SliderTrack = "Theme.Slider.Track";
    public const string SliderThumb = "Theme.Slider.Thumb";
    public const string SliderThumbHover = "Theme.Slider.ThumbHover";
    public const string CheckBoxBackground = "Theme.CheckBox.Background";
    public const string CheckBoxBorder = "Theme.CheckBox.Border";
    public const string CheckBoxGlyph = "Theme.CheckBox.Glyph";
    public const string CheckBoxCheckedBackground = "Theme.CheckBox.CheckedBackground";
    public const string GroupBoxHeaderForeground = "Theme.GroupBox.HeaderForeground";

    // Everything else
    public const string SplitterBackground = "Theme.Splitter.Background";
    public const string SplitterHoverBackground = "Theme.Splitter.HoverBackground";
    public const string MarqueeFill = "Theme.Marquee.Fill";
    public const string MarqueeBorder = "Theme.Marquee.Border";
    public const string ThumbnailTileBackground = "Theme.Thumbnail.TileBackground";

    private const string GroupSurfaces = "Surfaces";
    private const string GroupText = "Text";
    private const string GroupBorders = "Borders";
    private const string GroupAccent = "Accent";
    private const string GroupLists = "Lists";
    private const string GroupTree = "Folder tree";
    private const string GroupTabs = "Tabs";
    private const string GroupTitleBar = "Title bar";
    private const string GroupToolbar = "Toolbar";
    private const string GroupInputs = "Inputs";
    private const string GroupButtons = "Buttons";
    private const string GroupMenus = "Menus";
    private const string GroupScrollBars = "Scrollbars";
    private const string GroupStatusBar = "Status bar";
    private const string GroupFeedback = "Feedback";
    private const string GroupControls = "Controls";
    private const string GroupMisc = "Misc";

    /// <summary>In editor display order: groups appear in the order they first occur here.</summary>
    public static IReadOnlyList<ThemeTokenDescriptor> Descriptors { get; } = new ThemeTokenDescriptor[]
    {
        new(WindowBackground, GroupSurfaces, "Window background", "The file list and the window body.", true),
        new(SurfaceBackground, GroupSurfaces, "Panel background", "Sidebar, tab strip and dialog bodies.", true),
        new(SurfaceRaised, GroupSurfaces, "Raised background", "Toolbar strip.", false),
        new(OverlayBackground, GroupSurfaces, "Popup background", "Tooltips and dropdowns.", false),
        new(OverlayBorder, GroupSurfaces, "Popup border", "", false),
        new(ShadowColor, GroupSurfaces, "Shadow", "Drop shadow under popups and the pinned tree row; alpha is the shadow strength.", false),

        new(TextPrimary, GroupText, "Text", "Default foreground.", true),
        new(TextSecondary, GroupText, "Secondary text", "Sizes, dates, relative folders.", true),
        new(TextMuted, GroupText, "Muted text", "Section headers and hints.", false),
        new(TextDisabled, GroupText, "Disabled text", "", false),
        new(TextOnAccent, GroupText, "Text on accent", "", false),
        new(TextLink, GroupText, "Link", "The undo link in the status bar.", false),
        new(TextPlaceholder, GroupText, "Placeholder", "The search box watermark.", false),

        new(BorderDefault, GroupBorders, "Border", "", true),
        new(BorderSubtle, GroupBorders, "Subtle border", "Separators and hairlines.", false),
        new(BorderFocus, GroupBorders, "Focus border", "The focused text field or control.", true),
        new(BorderActive, GroupBorders, "Active pane border", "Outlines the pane the next keystroke lands in.", false),

        new(AccentBackground, GroupAccent, "Accent", "Primary buttons.", true),
        new(AccentHoverBackground, GroupAccent, "Accent hover", "", false),
        new(AccentPressedBackground, GroupAccent, "Accent pressed", "", false),
        new(AccentForeground, GroupAccent, "Accent foreground", "", true),
        new(AccentMuted, GroupAccent, "Muted accent", "", false),

        new(ListBackground, GroupLists, "List background", "", false),
        new(ListHoverBackground, GroupLists, "Row hover", "", true),
        new(ListSelectedBackground, GroupLists, "Row selected", "", true),
        new(ListSelectedForeground, GroupLists, "Row selected text", "", true),
        new(ListSelectedHoverBackground, GroupLists, "Row selected + hover", "", false),
        new(ListSelectedInactiveBackground, GroupLists, "Row selected (unfocused)", "", false),
        new(ListSelectedInactiveForeground, GroupLists, "Row selected text (unfocused)", "", false),
        new(ListDropBackground, GroupLists, "Drop target", "The folder a drag is hovering.", false),
        new(ListDropBorder, GroupLists, "Drop target border", "", false),
        new(ListHeaderBackground, GroupLists, "Column header", "", false),
        new(ListHeaderForeground, GroupLists, "Column header text", "", false),
        new(ListHeaderHoverBackground, GroupLists, "Column header hover", "", false),
        new(ListHeaderBorder, GroupLists, "Column header border", "", false),

        new(TreeChevronForeground, GroupTree, "Expander chevron", "", false),
        new(TreeChevronHoverForeground, GroupTree, "Expander chevron hover", "", false),

        new(TabStripBackground, GroupTabs, "Tab strip", "", false),
        new(TabActiveBackground, GroupTabs, "Active tab", "", true),
        new(TabActiveForeground, GroupTabs, "Active tab text", "", false),
        new(TabInactiveBackground, GroupTabs, "Inactive tab", "", true),
        new(TabInactiveForeground, GroupTabs, "Inactive tab text", "", false),
        new(TabHoverBackground, GroupTabs, "Tab hover", "", false),
        new(TabBorder, GroupTabs, "Tab border", "", false),
        new(TabActiveIndicator, GroupTabs, "Active tab indicator", "The stripe along the top of the active tab.", false),

        new(TitleBarBackground, GroupTitleBar, "Title bar", "", true),
        new(TitleBarInactiveBackground, GroupTitleBar, "Title bar (unfocused)", "", false),
        new(TitleBarForeground, GroupTitleBar, "Title bar text", "", false),
        new(TitleBarInactiveForeground, GroupTitleBar, "Title bar text (unfocused)", "", false),
        new(TitleBarButtonHoverBackground, GroupTitleBar, "Caption button hover", "", false),
        new(TitleBarButtonPressedBackground, GroupTitleBar, "Caption button pressed", "", false),
        new(TitleBarCloseHoverBackground, GroupTitleBar, "Close button hover", "", false),
        new(TitleBarCloseHoverForeground, GroupTitleBar, "Close button hover glyph", "", false),
        new(TitleBarBorder, GroupTitleBar, "Window border", "", false),

        new(ToolbarBackground, GroupToolbar, "Toolbar", "", false),
        new(ToolbarForeground, GroupToolbar, "Toolbar icons", "", false),
        new(ToolbarHoverBackground, GroupToolbar, "Toolbar button hover", "", false),
        new(ToolbarPressedBackground, GroupToolbar, "Toolbar button pressed", "", false),
        new(ToolbarCheckedForeground, GroupToolbar, "Toolbar toggle on", "The show-hidden-items toggle when active.", false),

        new(InputBackground, GroupInputs, "Field background", "Address bar and search box.", false),
        new(InputForeground, GroupInputs, "Field text", "", false),
        new(InputBorder, GroupInputs, "Field border", "", false),
        new(InputSelectionBackground, GroupInputs, "Text selection", "", false),
        new(InputSelectionForeground, GroupInputs, "Selected text", "", false),
        new(InputCaret, GroupInputs, "Caret", "", false),

        new(ButtonSecondaryBackground, GroupButtons, "Button", "", false),
        new(ButtonSecondaryHoverBackground, GroupButtons, "Button hover", "", false),
        new(ButtonSecondaryPressedBackground, GroupButtons, "Button pressed", "", false),
        new(ButtonSecondaryForeground, GroupButtons, "Button text", "", false),
        new(ButtonDisabledBackground, GroupButtons, "Button disabled", "", false),
        new(ButtonDisabledForeground, GroupButtons, "Button disabled text", "", false),

        new(MenuBackground, GroupMenus, "Menu background", "", false),
        new(MenuForeground, GroupMenus, "Menu text", "", false),
        new(MenuBorder, GroupMenus, "Menu border", "", false),
        new(MenuHoverBackground, GroupMenus, "Menu item hover", "", false),
        new(MenuHoverForeground, GroupMenus, "Menu item hover text", "", false),
        new(MenuSeparator, GroupMenus, "Menu separator", "", false),
        new(MenuIconForeground, GroupMenus, "Menu icon", "", false),
        new(MenuGestureForeground, GroupMenus, "Menu shortcut text", "", false),
        new(MenuDisabledForeground, GroupMenus, "Menu disabled text", "", false),

        new(ScrollBarBackground, GroupScrollBars, "Scrollbar track", "Usually transparent, so the bar floats over content.", false),
        new(ScrollBarThumb, GroupScrollBars, "Scrollbar thumb", "", true),
        new(ScrollBarThumbHover, GroupScrollBars, "Scrollbar thumb hover", "", false),
        new(ScrollBarThumbActive, GroupScrollBars, "Scrollbar thumb dragging", "", false),

        new(StatusBarBackground, GroupStatusBar, "Status bar", "", true),
        new(StatusBarForeground, GroupStatusBar, "Status bar text", "", false),
        new(StatusBarMutedForeground, GroupStatusBar, "Status bar secondary text", "", false),
        new(StatusBarHoverBackground, GroupStatusBar, "Status bar item hover", "", false),

        new(ErrorForeground, GroupFeedback, "Error text", "", false),
        new(ErrorBackground, GroupFeedback, "Error background", "", false),
        new(ErrorBorder, GroupFeedback, "Error border", "", false),
        new(WarningForeground, GroupFeedback, "Warning text", "", false),
        new(WarningBackground, GroupFeedback, "Warning background", "The banner above a folder that failed to list.", false),
        new(WarningBorder, GroupFeedback, "Warning border", "", false),
        new(ProgressIndicator, GroupFeedback, "Progress bar", "", false),
        new(ProgressTrack, GroupFeedback, "Progress track", "", false),

        new(SliderTrack, GroupControls, "Slider track", "", false),
        new(SliderThumb, GroupControls, "Slider thumb", "", false),
        new(SliderThumbHover, GroupControls, "Slider thumb hover", "", false),
        new(CheckBoxBackground, GroupControls, "Checkbox", "", false),
        new(CheckBoxBorder, GroupControls, "Checkbox border", "", false),
        new(CheckBoxGlyph, GroupControls, "Checkbox tick", "", false),
        new(CheckBoxCheckedBackground, GroupControls, "Checkbox checked", "", false),
        new(GroupBoxHeaderForeground, GroupControls, "Group header", "", false),

        new(SplitterBackground, GroupMisc, "Splitter", "", false),
        new(SplitterHoverBackground, GroupMisc, "Splitter hover", "", false),
        new(MarqueeFill, GroupMisc, "Rubber band fill", "The drag-to-select rectangle; alpha matters here.", false),
        new(MarqueeBorder, GroupMisc, "Rubber band border", "", false),
        new(ThumbnailTileBackground, GroupMisc, "Thumbnail tile", "Sits behind previews so transparent images still read.", false),
    };

    /// <summary>Every token key, in <see cref="Descriptors"/> order.</summary>
    public static IReadOnlyList<string> All { get; } = Descriptors.Select(d => d.Key).ToArray();

    private static readonly Dictionary<string, ThemeTokenDescriptor> ByKey =
        Descriptors.ToDictionary(d => d.Key, StringComparer.Ordinal);

    public static bool IsKnown(string key) => ByKey.ContainsKey(key);

    public static ThemeTokenDescriptor Describe(string key) => ByKey[key];

    public static ThemeTokenDescriptor? TryDescribe(string key) =>
        ByKey.TryGetValue(key, out var descriptor) ? descriptor : null;
}
