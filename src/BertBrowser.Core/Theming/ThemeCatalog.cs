using T = BertBrowser.Core.Theming.ThemeToken;

namespace BertBrowser.Core.Theming;

/// <summary>
/// The themes that ship with the app, as data rather than as files, so every colour we publish is
/// visible to the test suite — <c>ThemeCatalogTests</c> checks completeness, parseability and text
/// contrast on all of them.
/// </summary>
/// <remarks>
/// <see cref="DarkPlus"/> and <see cref="LightPlus"/> are roots and define every token in
/// <see cref="ThemeToken.All"/>. The rest are sparse sheets over Dark+, resolved through exactly the
/// same inheritance path a user's own theme uses — so that path is exercised by everything we ship.
/// </remarks>
public static class ThemeCatalog
{
    /// <summary>
    /// One blue for every surface that carries white text (status bar, menu highlight, primary
    /// buttons). VS Code's own #007ACC sits at 4.5:1 against white — right on the WCAG AA line — so
    /// this is a shade darker and comfortably clears it while reading as the same colour.
    /// </summary>
    private const string StrongBlue = "#0F6CBD";

    private const string StrongBlueHover = "#0A5A9E";
    private const string StrongBluePressed = "#084B85";

    public static ThemeDefinition DarkPlus { get; } = new()
    {
        Id = "dark-plus",
        Name = "Dark+",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#1E1E1E",
            [T.SurfaceBackground] = "#252526",
            [T.SurfaceRaised] = "#333333",
            [T.OverlayBackground] = "#252526",
            [T.OverlayBorder] = "#454545",
            [T.ShadowColor] = "#99000000",

            [T.TextPrimary] = "#CCCCCC",
            [T.TextSecondary] = "#9D9D9D",
            [T.TextMuted] = "#7A7A7A",
            [T.TextDisabled] = "#6E6E6E",
            [T.TextOnAccent] = "#FFFFFF",
            [T.TextLink] = "#3794FF",
            [T.TextPlaceholder] = "#A6A6A6",

            [T.BorderDefault] = "#3C3C3C",
            [T.BorderSubtle] = "#2B2B2B",
            [T.BorderFocus] = "#007FD4",
            [T.BorderActive] = "#007ACC",

            [T.AccentBackground] = "#0E639C",
            [T.AccentHoverBackground] = "#1177BB",
            [T.AccentPressedBackground] = "#0D5A8E",
            [T.AccentForeground] = "#FFFFFF",
            [T.AccentMuted] = "#04395E",

            [T.ListBackground] = "#1E1E1E",
            [T.ListHoverBackground] = "#2A2D2E",
            [T.ListSelectedBackground] = "#04395E",
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = "#0A4A75",
            [T.ListSelectedInactiveBackground] = "#37373D",
            [T.ListSelectedInactiveForeground] = "#CCCCCC",
            [T.ListDropBackground] = "#062F4A",
            [T.ListDropBorder] = "#007ACC",
            [T.ListHeaderBackground] = "#252526",
            [T.ListHeaderForeground] = "#CCCCCC",
            [T.ListHeaderHoverBackground] = "#2A2D2E",
            [T.ListHeaderBorder] = "#3C3C3C",

            [T.TreeChevronForeground] = "#C5C5C5",
            [T.TreeChevronHoverForeground] = "#E7E7E7",

            [T.TabStripBackground] = "#252526",
            [T.TabActiveBackground] = "#1E1E1E",
            [T.TabActiveForeground] = "#FFFFFF",
            [T.TabInactiveBackground] = "#2D2D2D",
            [T.TabInactiveForeground] = "#9D9D9D",
            [T.TabHoverBackground] = "#333333",
            [T.TabBorder] = "#252526",
            [T.TabActiveIndicator] = "#007ACC",

            [T.TitleBarBackground] = "#3C3C3C",
            [T.TitleBarInactiveBackground] = "#3C3C3C",
            [T.TitleBarForeground] = "#CCCCCC",
            [T.TitleBarInactiveForeground] = "#8A8A8A",
            [T.TitleBarButtonHoverBackground] = "#4D4D4D",
            [T.TitleBarButtonPressedBackground] = "#5A5A5A",
            [T.TitleBarCloseHoverBackground] = "#C42B1C",
            [T.TitleBarCloseHoverForeground] = "#FFFFFF",
            [T.TitleBarBorder] = "#2B2B2B",

            [T.ToolbarBackground] = "#333333",
            [T.ToolbarForeground] = "#CCCCCC",
            [T.ToolbarHoverBackground] = "#4F5254",
            [T.ToolbarPressedBackground] = "#5A5D5E",
            [T.ToolbarCheckedForeground] = "#4EC9F5",

            [T.InputBackground] = "#3C3C3C",
            [T.InputForeground] = "#CCCCCC",
            [T.InputBorder] = "#3C3C3C",
            [T.InputSelectionBackground] = "#264F78",
            [T.InputSelectionForeground] = "#FFFFFF",
            [T.InputCaret] = "#AEAFAD",

            [T.ButtonSecondaryBackground] = "#3A3D41",
            [T.ButtonSecondaryHoverBackground] = "#45494E",
            [T.ButtonSecondaryPressedBackground] = "#4F5459",
            [T.ButtonSecondaryForeground] = "#CCCCCC",
            [T.ButtonDisabledBackground] = "#2D2D2D",
            [T.ButtonDisabledForeground] = "#6E6E6E",

            [T.MenuBackground] = "#252526",
            [T.MenuForeground] = "#CCCCCC",
            [T.MenuBorder] = "#454545",
            [T.MenuHoverBackground] = StrongBlue,
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#454545",
            [T.MenuIconForeground] = "#C5C5C5",
            [T.MenuGestureForeground] = "#8A8A8A",
            [T.MenuDisabledForeground] = "#6E6E6E",

            [T.ScrollBarBackground] = "#00000000",
            [T.ScrollBarThumb] = "#79797966",
            [T.ScrollBarThumbHover] = "#646464B3",
            [T.ScrollBarThumbActive] = "#BFBFBF66",

            [T.StatusBarBackground] = StrongBlue,
            [T.StatusBarForeground] = "#FFFFFF",
            [T.StatusBarMutedForeground] = "#FFFFFFCC",
            [T.StatusBarHoverBackground] = "#FFFFFF1F",

            [T.ErrorForeground] = "#F48771",
            [T.ErrorBackground] = "#5A1D1D",
            [T.ErrorBorder] = "#BE1100",
            [T.WarningForeground] = "#CCA700",
            [T.WarningBackground] = "#352A05",
            [T.WarningBorder] = "#B89500",
            [T.ProgressIndicator] = "#0E70C0",
            [T.ProgressTrack] = "#2D2D2D",

            [T.SliderTrack] = "#4D4D4D",
            [T.SliderThumb] = "#CCCCCC",
            [T.SliderThumbHover] = "#FFFFFF",
            [T.CheckBoxBackground] = "#3C3C3C",
            [T.CheckBoxBorder] = "#6E6E6E",
            [T.CheckBoxGlyph] = "#FFFFFF",
            [T.CheckBoxCheckedBackground] = "#0E639C",
            [T.GroupBoxHeaderForeground] = "#CCCCCC",

            [T.SplitterBackground] = "#00000000",
            [T.SplitterHoverBackground] = "#007ACC",
            [T.MarqueeFill] = "#33007ACC",
            [T.MarqueeBorder] = "#007ACC",
            [T.ThumbnailTileBackground] = "#2A2A2A",
        },
    };

    public static ThemeDefinition LightPlus { get; } = new()
    {
        Id = "light-plus",
        Name = "Light+",
        IsDark = false,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#FFFFFF",
            [T.SurfaceBackground] = "#F3F3F3",
            [T.SurfaceRaised] = "#ECECEC",
            [T.OverlayBackground] = "#FFFFFF",
            [T.OverlayBorder] = "#D4D4D4",
            [T.ShadowColor] = "#29000000",

            [T.TextPrimary] = "#1F1F1F",
            [T.TextSecondary] = "#616161",
            [T.TextMuted] = "#767676",
            [T.TextDisabled] = "#A0A0A0",
            [T.TextOnAccent] = "#FFFFFF",
            [T.TextLink] = "#0066BF",
            [T.TextPlaceholder] = "#767676",

            [T.BorderDefault] = "#CECECE",
            [T.BorderSubtle] = "#E5E5E5",
            [T.BorderFocus] = "#0090F1",
            [T.BorderActive] = "#007ACC",

            [T.AccentBackground] = StrongBlue,
            [T.AccentHoverBackground] = StrongBlueHover,
            [T.AccentPressedBackground] = StrongBluePressed,
            [T.AccentForeground] = "#FFFFFF",
            [T.AccentMuted] = "#CCE4F7",

            [T.ListBackground] = "#FFFFFF",
            [T.ListHoverBackground] = "#E8E8E8",
            [T.ListSelectedBackground] = StrongBlue,
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = StrongBlueHover,
            [T.ListSelectedInactiveBackground] = "#E4E6F1",
            [T.ListSelectedInactiveForeground] = "#1F1F1F",
            [T.ListDropBackground] = "#D6EBFF",
            [T.ListDropBorder] = "#0090F1",
            [T.ListHeaderBackground] = "#F3F3F3",
            [T.ListHeaderForeground] = "#4A4A4A",
            [T.ListHeaderHoverBackground] = "#E8E8E8",
            [T.ListHeaderBorder] = "#DDDDDD",

            [T.TreeChevronForeground] = "#6E6E6E",
            [T.TreeChevronHoverForeground] = "#007ACC",

            [T.TabStripBackground] = "#F3F3F3",
            [T.TabActiveBackground] = "#FFFFFF",
            [T.TabActiveForeground] = "#333333",
            [T.TabInactiveBackground] = "#ECECEC",
            [T.TabInactiveForeground] = "#6E6E6E",
            [T.TabHoverBackground] = "#E3E3E3",
            [T.TabBorder] = "#E0E0E0",
            [T.TabActiveIndicator] = "#007ACC",

            [T.TitleBarBackground] = "#DDDDDD",
            [T.TitleBarInactiveBackground] = "#E8E8E8",
            [T.TitleBarForeground] = "#333333",
            [T.TitleBarInactiveForeground] = "#7A7A7A",
            [T.TitleBarButtonHoverBackground] = "#CFCFCF",
            [T.TitleBarButtonPressedBackground] = "#BFBFBF",
            [T.TitleBarCloseHoverBackground] = "#C42B1C",
            [T.TitleBarCloseHoverForeground] = "#FFFFFF",
            [T.TitleBarBorder] = "#C8C8C8",

            [T.ToolbarBackground] = "#F3F3F3",
            [T.ToolbarForeground] = "#444444",
            [T.ToolbarHoverBackground] = "#DCDCDC",
            [T.ToolbarPressedBackground] = "#C8C8C8",
            [T.ToolbarCheckedForeground] = StrongBlue,

            [T.InputBackground] = "#FFFFFF",
            [T.InputForeground] = "#1F1F1F",
            [T.InputBorder] = "#CECECE",
            [T.InputSelectionBackground] = "#ADD6FF",
            [T.InputSelectionForeground] = "#1F1F1F",
            [T.InputCaret] = "#000000",

            [T.ButtonSecondaryBackground] = "#E4E4E4",
            [T.ButtonSecondaryHoverBackground] = "#D6D6D6",
            [T.ButtonSecondaryPressedBackground] = "#C8C8C8",
            [T.ButtonSecondaryForeground] = "#1F1F1F",
            [T.ButtonDisabledBackground] = "#EBEBEB",
            [T.ButtonDisabledForeground] = "#A0A0A0",

            [T.MenuBackground] = "#FFFFFF",
            [T.MenuForeground] = "#1F1F1F",
            [T.MenuBorder] = "#D4D4D4",
            [T.MenuHoverBackground] = StrongBlue,
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#E0E0E0",
            [T.MenuIconForeground] = "#444444",
            [T.MenuGestureForeground] = "#767676",
            [T.MenuDisabledForeground] = "#A0A0A0",

            [T.ScrollBarBackground] = "#00000000",
            [T.ScrollBarThumb] = "#64646466",
            [T.ScrollBarThumbHover] = "#646464B3",
            [T.ScrollBarThumbActive] = "#00000099",

            [T.StatusBarBackground] = StrongBlue,
            [T.StatusBarForeground] = "#FFFFFF",
            [T.StatusBarMutedForeground] = "#FFFFFFCC",
            [T.StatusBarHoverBackground] = "#FFFFFF1F",

            [T.ErrorForeground] = "#A31515",
            [T.ErrorBackground] = "#FDE7E9",
            [T.ErrorBorder] = "#E51400",
            [T.WarningForeground] = "#7A5C00",
            [T.WarningBackground] = "#FFF4E5",
            [T.WarningBorder] = "#E6C079",
            [T.ProgressIndicator] = StrongBlue,
            [T.ProgressTrack] = "#E6E6E6",

            [T.SliderTrack] = "#C8C8C8",
            [T.SliderThumb] = StrongBlue,
            [T.SliderThumbHover] = StrongBlueHover,
            [T.CheckBoxBackground] = "#FFFFFF",
            [T.CheckBoxBorder] = "#767676",
            [T.CheckBoxGlyph] = "#FFFFFF",
            [T.CheckBoxCheckedBackground] = StrongBlue,
            [T.GroupBoxHeaderForeground] = "#1F1F1F",

            [T.SplitterBackground] = "#00000000",
            [T.SplitterHoverBackground] = "#0090F1",
            [T.MarqueeFill] = "#330F6CBD",
            [T.MarqueeBorder] = StrongBlue,
            [T.ThumbnailTileBackground] = "#00000000",
        },
    };

    public static ThemeDefinition Monokai { get; } = new()
    {
        Id = "monokai",
        Name = "Monokai",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#272822",
            [T.SurfaceBackground] = "#1E1F1C",
            [T.SurfaceRaised] = "#34352F",
            [T.OverlayBackground] = "#1E1F1C",
            [T.OverlayBorder] = "#5A5B54",

            [T.TextPrimary] = "#F8F8F2",
            [T.TextSecondary] = "#B4B392",
            [T.TextMuted] = "#96958A",
            [T.TextLink] = "#66D9EF",
            [T.TextPlaceholder] = "#B4B392",

            [T.BorderDefault] = "#4A4B44",
            [T.BorderSubtle] = "#34352F",
            [T.BorderFocus] = "#A6E22E",
            [T.BorderActive] = "#A6E22E",

            [T.AccentBackground] = "#5B5A45",
            [T.AccentHoverBackground] = "#6E6C53",
            [T.AccentPressedBackground] = "#4B4A38",
            [T.AccentMuted] = "#3E3D32",

            [T.ListBackground] = "#272822",
            [T.ListHoverBackground] = "#3E3D32",
            [T.ListSelectedBackground] = "#49483E",
            [T.ListSelectedForeground] = "#F8F8F2",
            [T.ListSelectedHoverBackground] = "#5A594B",
            [T.ListSelectedInactiveBackground] = "#3E3D32",
            [T.ListSelectedInactiveForeground] = "#F8F8F2",
            [T.ListDropBackground] = "#3E4A22",
            [T.ListDropBorder] = "#A6E22E",
            [T.ListHeaderBackground] = "#1E1F1C",
            [T.ListHeaderForeground] = "#F8F8F2",
            [T.ListHeaderHoverBackground] = "#3E3D32",
            [T.ListHeaderBorder] = "#4A4B44",

            [T.TabStripBackground] = "#1E1F1C",
            [T.TabActiveBackground] = "#272822",
            [T.TabActiveForeground] = "#F8F8F2",
            [T.TabInactiveBackground] = "#34352F",
            [T.TabInactiveForeground] = "#96958A",
            [T.TabHoverBackground] = "#3E3D32",
            [T.TabBorder] = "#1E1F1C",
            [T.TabActiveIndicator] = "#A6E22E",

            [T.TitleBarBackground] = "#1E1F1C",
            [T.TitleBarInactiveBackground] = "#1E1F1C",
            [T.TitleBarForeground] = "#F8F8F2",
            [T.TitleBarBorder] = "#141512",

            [T.ToolbarBackground] = "#34352F",
            [T.ToolbarForeground] = "#F8F8F2",
            [T.ToolbarHoverBackground] = "#4A4B44",
            [T.ToolbarCheckedForeground] = "#A6E22E",

            [T.InputBackground] = "#1E1F1C",
            [T.InputForeground] = "#F8F8F2",
            [T.InputBorder] = "#4A4B44",
            [T.InputSelectionBackground] = "#49483E",
            [T.InputCaret] = "#F8F8F0",

            [T.MenuBackground] = "#1E1F1C",
            [T.MenuForeground] = "#F8F8F2",
            [T.MenuBorder] = "#5A5B54",
            [T.MenuHoverBackground] = "#49483E",
            [T.MenuHoverForeground] = "#F8F8F2",
            [T.MenuSeparator] = "#4A4B44",
            [T.MenuIconForeground] = "#B4B392",

            [T.StatusBarBackground] = "#4B4A38",
            [T.StatusBarForeground] = "#F8F8F2",

            [T.ErrorForeground] = "#F92672",
            [T.WarningForeground] = "#E6DB74",
            [T.ProgressIndicator] = "#A6E22E",

            [T.SliderThumb] = "#A6E22E",
            [T.CheckBoxCheckedBackground] = "#5B5A45",
            [T.SplitterHoverBackground] = "#A6E22E",
            [T.MarqueeFill] = "#33A6E22E",
            [T.MarqueeBorder] = "#A6E22E",
            [T.ThumbnailTileBackground] = "#34352F",
        },
    };

    public static ThemeDefinition SolarizedDark { get; } = new()
    {
        Id = "solarized-dark",
        Name = "Solarized Dark",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#002B36",
            [T.SurfaceBackground] = "#00212B",
            [T.SurfaceRaised] = "#073642",
            [T.OverlayBackground] = "#00212B",
            [T.OverlayBorder] = "#11565F",

            [T.TextPrimary] = "#93A1A1",
            [T.TextSecondary] = "#839496",
            [T.TextMuted] = "#657B83",
            [T.TextLink] = "#268BD2",
            [T.TextPlaceholder] = "#839496",

            [T.BorderDefault] = "#0B4453",
            [T.BorderSubtle] = "#073642",
            [T.BorderFocus] = "#268BD2",
            [T.BorderActive] = "#268BD2",

            [T.AccentBackground] = "#1F6F9E",
            [T.AccentHoverBackground] = "#2A82B5",
            [T.AccentPressedBackground] = "#175A82",
            [T.AccentMuted] = "#004052",

            [T.ListBackground] = "#002B36",
            // Solarized's own base02 is the selection tone, so hover sits a step below it: any
            // lighter and base1 text on a hovered row drops under 4.5:1.
            [T.ListHoverBackground] = "#01323F",
            [T.ListSelectedBackground] = "#00596F",
            [T.ListSelectedForeground] = "#FDF6E3",
            [T.ListSelectedHoverBackground] = "#006C86",
            [T.ListSelectedInactiveBackground] = "#073642",
            [T.ListSelectedInactiveForeground] = "#93A1A1",
            [T.ListDropBackground] = "#00485C",
            [T.ListDropBorder] = "#268BD2",
            [T.ListHeaderBackground] = "#00212B",
            [T.ListHeaderForeground] = "#93A1A1",
            [T.ListHeaderHoverBackground] = "#01323F",
            [T.ListHeaderBorder] = "#0B4453",

            [T.TabStripBackground] = "#00212B",
            [T.TabActiveBackground] = "#002B36",
            [T.TabActiveForeground] = "#FDF6E3",
            [T.TabInactiveBackground] = "#073642",
            [T.TabInactiveForeground] = "#839496",
            [T.TabHoverBackground] = "#01323F",
            [T.TabBorder] = "#00212B",
            [T.TabActiveIndicator] = "#268BD2",

            [T.TitleBarBackground] = "#073642",
            [T.TitleBarInactiveBackground] = "#073642",
            [T.TitleBarForeground] = "#93A1A1",
            [T.TitleBarBorder] = "#00171E",

            [T.ToolbarBackground] = "#073642",
            [T.ToolbarForeground] = "#93A1A1",
            [T.ToolbarHoverBackground] = "#0B4453",
            [T.ToolbarCheckedForeground] = "#2AA198",

            [T.InputBackground] = "#00212B",
            [T.InputForeground] = "#93A1A1",
            [T.InputBorder] = "#0B4453",
            [T.InputSelectionBackground] = "#00596F",
            [T.InputCaret] = "#93A1A1",

            [T.MenuBackground] = "#00212B",
            [T.MenuForeground] = "#93A1A1",
            [T.MenuBorder] = "#11565F",
            [T.MenuHoverBackground] = "#1F6F9E",
            [T.MenuHoverForeground] = "#FDF6E3",
            [T.MenuSeparator] = "#0B4453",
            [T.MenuIconForeground] = "#839496",

            [T.StatusBarBackground] = "#1F6F9E",
            [T.StatusBarForeground] = "#FDF6E3",

            [T.ErrorForeground] = "#DC322F",
            [T.WarningForeground] = "#B58900",
            [T.ProgressIndicator] = "#268BD2",

            [T.SliderThumb] = "#268BD2",
            [T.CheckBoxCheckedBackground] = "#1F6F9E",
            [T.SplitterHoverBackground] = "#268BD2",
            [T.MarqueeFill] = "#33268BD2",
            [T.MarqueeBorder] = "#268BD2",
            [T.ThumbnailTileBackground] = "#073642",
        },
    };

    public static ThemeDefinition TokyoNight { get; } = new()
    {
        Id = "tokyo-night",
        Name = "Tokyo Night",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#1A1B26",
            [T.SurfaceBackground] = "#16161E",
            [T.SurfaceRaised] = "#1F2335",
            [T.OverlayBackground] = "#16161E",
            [T.OverlayBorder] = "#3B4261",

            [T.TextPrimary] = "#C0CAF5",
            [T.TextSecondary] = "#A9B1D6",
            // Tokyo Night's own comment grey (#565F89) lands at 3.0:1 on the sidebar — exactly on the
            // line — so muted text is a step brighter than the palette's.
            [T.TextMuted] = "#7C86B8",
            [T.TextLink] = "#7AA2F7",
            [T.TextPlaceholder] = "#7C86B8",

            [T.BorderDefault] = "#292E42",
            [T.BorderSubtle] = "#1F2335",
            [T.BorderFocus] = "#7AA2F7",
            [T.BorderActive] = "#7AA2F7",

            [T.AccentBackground] = "#3D59A1",
            [T.AccentHoverBackground] = "#4C6BC0",
            [T.AccentPressedBackground] = "#34497F",
            [T.AccentMuted] = "#283457",

            [T.ListBackground] = "#1A1B26",
            [T.ListHoverBackground] = "#222436",
            [T.ListSelectedBackground] = "#2E3C64",
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = "#3A4B7C",
            [T.ListSelectedInactiveBackground] = "#292E42",
            [T.ListSelectedInactiveForeground] = "#C0CAF5",
            [T.ListDropBackground] = "#24365E",
            [T.ListDropBorder] = "#7AA2F7",
            [T.ListHeaderBackground] = "#16161E",
            [T.ListHeaderForeground] = "#A9B1D6",
            [T.ListHeaderHoverBackground] = "#222436",
            [T.ListHeaderBorder] = "#292E42",

            [T.TreeChevronForeground] = "#A9B1D6",
            [T.TreeChevronHoverForeground] = "#7AA2F7",

            [T.TabStripBackground] = "#16161E",
            [T.TabActiveBackground] = "#1A1B26",
            [T.TabActiveForeground] = "#C0CAF5",
            [T.TabInactiveBackground] = "#1F2335",
            [T.TabInactiveForeground] = "#7C86B8",
            [T.TabHoverBackground] = "#292E42",
            [T.TabBorder] = "#16161E",
            [T.TabActiveIndicator] = "#7AA2F7",

            [T.TitleBarBackground] = "#16161E",
            [T.TitleBarInactiveBackground] = "#16161E",
            [T.TitleBarForeground] = "#C0CAF5",
            [T.TitleBarInactiveForeground] = "#7C86B8",
            [T.TitleBarButtonHoverBackground] = "#292E42",
            [T.TitleBarButtonPressedBackground] = "#3B4261",
            [T.TitleBarBorder] = "#0F0F17",

            [T.ToolbarBackground] = "#1F2335",
            [T.ToolbarForeground] = "#C0CAF5",
            [T.ToolbarHoverBackground] = "#2F3549",
            [T.ToolbarPressedBackground] = "#3B4261",
            [T.ToolbarCheckedForeground] = "#7DCFFF",

            [T.InputBackground] = "#16161E",
            [T.InputForeground] = "#C0CAF5",
            [T.InputBorder] = "#3B4261",
            [T.InputSelectionBackground] = "#2E3C64",
            [T.InputCaret] = "#C0CAF5",

            [T.ButtonSecondaryBackground] = "#292E42",
            [T.ButtonSecondaryHoverBackground] = "#343A54",
            [T.ButtonSecondaryPressedBackground] = "#3B4261",
            [T.ButtonSecondaryForeground] = "#C0CAF5",
            [T.ButtonDisabledBackground] = "#1F2335",

            [T.MenuBackground] = "#16161E",
            [T.MenuForeground] = "#C0CAF5",
            [T.MenuBorder] = "#3B4261",
            [T.MenuHoverBackground] = "#3D59A1",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#292E42",
            [T.MenuIconForeground] = "#A9B1D6",
            [T.MenuGestureForeground] = "#7C86B8",

            [T.ScrollBarThumb] = "#565F8999",
            [T.ScrollBarThumbHover] = "#7C86B8B3",
            [T.ScrollBarThumbActive] = "#A9B1D6",

            [T.StatusBarBackground] = "#3D59A1",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#F7768E",
            [T.ErrorBackground] = "#3B2431",
            [T.ErrorBorder] = "#DB4B6A",
            [T.WarningForeground] = "#E0AF68",
            [T.WarningBackground] = "#3A2F1B",
            [T.WarningBorder] = "#B08A45",
            [T.ProgressIndicator] = "#7AA2F7",
            [T.ProgressTrack] = "#292E42",

            [T.SliderTrack] = "#3B4261",
            [T.SliderThumb] = "#7AA2F7",
            [T.SliderThumbHover] = "#9EC1FF",
            [T.CheckBoxBackground] = "#16161E",
            [T.CheckBoxBorder] = "#565F89",
            [T.CheckBoxCheckedBackground] = "#3D59A1",
            [T.GroupBoxHeaderForeground] = "#C0CAF5",

            [T.SplitterHoverBackground] = "#7AA2F7",
            [T.MarqueeFill] = "#337AA2F7",
            [T.MarqueeBorder] = "#7AA2F7",
            [T.ThumbnailTileBackground] = "#1F2335",
        },
    };

    public static ThemeDefinition CatppuccinMocha { get; } = new()
    {
        Id = "catppuccin-mocha",
        Name = "Catppuccin Mocha",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#1E1E2E",
            [T.SurfaceBackground] = "#181825",
            [T.SurfaceRaised] = "#313244",
            [T.OverlayBackground] = "#181825",
            [T.OverlayBorder] = "#45475A",

            [T.TextPrimary] = "#CDD6F4",
            [T.TextSecondary] = "#BAC2DE",
            [T.TextMuted] = "#9399B2",
            [T.TextDisabled] = "#6C7086",
            [T.TextLink] = "#89B4FA",
            [T.TextPlaceholder] = "#9399B2",

            [T.BorderDefault] = "#45475A",
            [T.BorderSubtle] = "#313244",
            [T.BorderFocus] = "#89B4FA",
            [T.BorderActive] = "#CBA6F7",

            // Mauve as the palette ships it is a light pastel, so the accent is the same hue taken
            // dark enough to carry white text.
            [T.AccentBackground] = "#7C4FBF",
            [T.AccentHoverBackground] = "#8F5FD6",
            [T.AccentPressedBackground] = "#663FA0",
            [T.AccentMuted] = "#3B2F55",

            [T.ListBackground] = "#1E1E2E",
            [T.ListHoverBackground] = "#292A3C",
            [T.ListSelectedBackground] = "#45475A",
            [T.ListSelectedForeground] = "#CDD6F4",
            [T.ListSelectedHoverBackground] = "#545767",
            [T.ListSelectedInactiveBackground] = "#313244",
            [T.ListSelectedInactiveForeground] = "#BAC2DE",
            [T.ListDropBackground] = "#3B2F55",
            [T.ListDropBorder] = "#CBA6F7",
            [T.ListHeaderBackground] = "#181825",
            [T.ListHeaderForeground] = "#BAC2DE",
            [T.ListHeaderHoverBackground] = "#292A3C",
            [T.ListHeaderBorder] = "#45475A",

            [T.TreeChevronForeground] = "#BAC2DE",
            [T.TreeChevronHoverForeground] = "#89B4FA",

            [T.TabStripBackground] = "#181825",
            [T.TabActiveBackground] = "#1E1E2E",
            [T.TabActiveForeground] = "#CDD6F4",
            [T.TabInactiveBackground] = "#26273A",
            [T.TabInactiveForeground] = "#9399B2",
            [T.TabHoverBackground] = "#313244",
            [T.TabBorder] = "#181825",
            [T.TabActiveIndicator] = "#CBA6F7",

            [T.TitleBarBackground] = "#181825",
            [T.TitleBarInactiveBackground] = "#181825",
            [T.TitleBarForeground] = "#CDD6F4",
            [T.TitleBarInactiveForeground] = "#9399B2",
            [T.TitleBarButtonHoverBackground] = "#313244",
            [T.TitleBarButtonPressedBackground] = "#45475A",
            [T.TitleBarCloseHoverBackground] = "#F38BA8",
            [T.TitleBarCloseHoverForeground] = "#11111B",
            [T.TitleBarBorder] = "#11111B",

            [T.ToolbarBackground] = "#313244",
            [T.ToolbarForeground] = "#CDD6F4",
            [T.ToolbarHoverBackground] = "#45475A",
            [T.ToolbarPressedBackground] = "#585B70",
            [T.ToolbarCheckedForeground] = "#94E2D5",

            [T.InputBackground] = "#181825",
            [T.InputForeground] = "#CDD6F4",
            [T.InputBorder] = "#45475A",
            [T.InputSelectionBackground] = "#45475A",
            [T.InputSelectionForeground] = "#CDD6F4",
            [T.InputCaret] = "#F5E0DC",

            [T.ButtonSecondaryBackground] = "#313244",
            [T.ButtonSecondaryHoverBackground] = "#45475A",
            [T.ButtonSecondaryPressedBackground] = "#585B70",
            [T.ButtonSecondaryForeground] = "#CDD6F4",
            [T.ButtonDisabledBackground] = "#26273A",

            [T.MenuBackground] = "#181825",
            [T.MenuForeground] = "#CDD6F4",
            [T.MenuBorder] = "#45475A",
            [T.MenuHoverBackground] = "#7C4FBF",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#313244",
            [T.MenuIconForeground] = "#BAC2DE",
            [T.MenuGestureForeground] = "#9399B2",

            [T.ScrollBarThumb] = "#6C708699",
            [T.ScrollBarThumbHover] = "#7F849CCC",
            [T.ScrollBarThumbActive] = "#9399B2",

            [T.StatusBarBackground] = "#7C4FBF",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#F38BA8",
            [T.ErrorBackground] = "#3B2430",
            [T.ErrorBorder] = "#F38BA8",
            [T.WarningForeground] = "#F9E2AF",
            [T.WarningBackground] = "#3B3527",
            [T.WarningBorder] = "#DFC28C",
            [T.ProgressIndicator] = "#CBA6F7",
            [T.ProgressTrack] = "#313244",

            [T.SliderTrack] = "#45475A",
            [T.SliderThumb] = "#CBA6F7",
            [T.SliderThumbHover] = "#DCC0FF",
            [T.CheckBoxBackground] = "#181825",
            [T.CheckBoxBorder] = "#6C7086",
            [T.CheckBoxCheckedBackground] = "#7C4FBF",
            [T.GroupBoxHeaderForeground] = "#CDD6F4",

            [T.SplitterHoverBackground] = "#CBA6F7",
            [T.MarqueeFill] = "#33CBA6F7",
            [T.MarqueeBorder] = "#CBA6F7",
            [T.ThumbnailTileBackground] = "#313244",
        },
    };

    public static ThemeDefinition Dracula { get; } = new()
    {
        Id = "dracula",
        Name = "Dracula",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#282A36",
            [T.SurfaceBackground] = "#21222C",
            [T.SurfaceRaised] = "#343746",
            [T.OverlayBackground] = "#21222C",
            [T.OverlayBorder] = "#6272A4",

            [T.TextPrimary] = "#F8F8F2",
            [T.TextSecondary] = "#BFC7E0",
            [T.TextMuted] = "#7F8BC0",
            [T.TextDisabled] = "#6272A4",
            [T.TextLink] = "#8BE9FD",
            [T.TextPlaceholder] = "#7F8BC0",

            [T.BorderDefault] = "#44475A",
            [T.BorderSubtle] = "#343746",
            [T.BorderFocus] = "#BD93F9",
            [T.BorderActive] = "#BD93F9",

            [T.AccentBackground] = "#6B4FA8",
            [T.AccentHoverBackground] = "#7C5DC4",
            [T.AccentPressedBackground] = "#58408C",
            [T.AccentMuted] = "#3B3352",

            [T.ListBackground] = "#282A36",
            [T.ListHoverBackground] = "#313442",
            [T.ListSelectedBackground] = "#44475A",
            [T.ListSelectedForeground] = "#F8F8F2",
            [T.ListSelectedHoverBackground] = "#52556B",
            [T.ListSelectedInactiveBackground] = "#343746",
            [T.ListSelectedInactiveForeground] = "#F8F8F2",
            [T.ListDropBackground] = "#3B2F52",
            [T.ListDropBorder] = "#BD93F9",
            [T.ListHeaderBackground] = "#21222C",
            [T.ListHeaderForeground] = "#BFC7E0",
            [T.ListHeaderHoverBackground] = "#313442",
            [T.ListHeaderBorder] = "#44475A",

            [T.TreeChevronForeground] = "#BFC7E0",
            [T.TreeChevronHoverForeground] = "#BD93F9",

            [T.TabStripBackground] = "#21222C",
            [T.TabActiveBackground] = "#282A36",
            [T.TabActiveForeground] = "#F8F8F2",
            [T.TabInactiveBackground] = "#2E303C",
            [T.TabInactiveForeground] = "#7F8BC0",
            [T.TabHoverBackground] = "#343746",
            [T.TabBorder] = "#21222C",
            [T.TabActiveIndicator] = "#FF79C6",

            [T.TitleBarBackground] = "#21222C",
            [T.TitleBarInactiveBackground] = "#21222C",
            [T.TitleBarForeground] = "#F8F8F2",
            [T.TitleBarInactiveForeground] = "#7F8BC0",
            [T.TitleBarButtonHoverBackground] = "#343746",
            [T.TitleBarButtonPressedBackground] = "#44475A",
            [T.TitleBarBorder] = "#16171E",

            [T.ToolbarBackground] = "#343746",
            [T.ToolbarForeground] = "#F8F8F2",
            [T.ToolbarHoverBackground] = "#44475A",
            [T.ToolbarPressedBackground] = "#52556B",
            [T.ToolbarCheckedForeground] = "#50FA7B",

            [T.InputBackground] = "#21222C",
            [T.InputForeground] = "#F8F8F2",
            [T.InputBorder] = "#44475A",
            [T.InputSelectionBackground] = "#44475A",
            [T.InputSelectionForeground] = "#F8F8F2",
            [T.InputCaret] = "#F8F8F2",

            [T.ButtonSecondaryBackground] = "#44475A",
            [T.ButtonSecondaryHoverBackground] = "#52556B",
            [T.ButtonSecondaryPressedBackground] = "#5D6178",
            [T.ButtonSecondaryForeground] = "#F8F8F2",
            [T.ButtonDisabledBackground] = "#2E303C",

            [T.MenuBackground] = "#21222C",
            [T.MenuForeground] = "#F8F8F2",
            [T.MenuBorder] = "#44475A",
            [T.MenuHoverBackground] = "#6B4FA8",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#44475A",
            [T.MenuIconForeground] = "#BFC7E0",
            [T.MenuGestureForeground] = "#7F8BC0",

            [T.ScrollBarThumb] = "#6272A499",
            [T.ScrollBarThumbHover] = "#8B93C4B3",
            [T.ScrollBarThumbActive] = "#BFC7E0",

            [T.StatusBarBackground] = "#6B4FA8",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#FF5555",
            [T.ErrorBackground] = "#3F2334",
            [T.ErrorBorder] = "#FF5555",
            [T.WarningForeground] = "#F1FA8C",
            [T.WarningBackground] = "#3A3A22",
            [T.WarningBorder] = "#C6CE6E",
            [T.ProgressIndicator] = "#BD93F9",
            [T.ProgressTrack] = "#343746",

            [T.SliderTrack] = "#44475A",
            [T.SliderThumb] = "#BD93F9",
            [T.SliderThumbHover] = "#D0B0FF",
            [T.CheckBoxBackground] = "#21222C",
            [T.CheckBoxBorder] = "#6272A4",
            [T.CheckBoxCheckedBackground] = "#6B4FA8",
            [T.GroupBoxHeaderForeground] = "#F8F8F2",

            [T.SplitterHoverBackground] = "#BD93F9",
            [T.MarqueeFill] = "#33BD93F9",
            [T.MarqueeBorder] = "#BD93F9",
            [T.ThumbnailTileBackground] = "#343746",
        },
    };

    public static ThemeDefinition Nord { get; } = new()
    {
        Id = "nord",
        Name = "Nord",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#2E3440",
            [T.SurfaceBackground] = "#272C36",
            [T.SurfaceRaised] = "#3B4252",
            [T.OverlayBackground] = "#2E3440",
            [T.OverlayBorder] = "#4C566A",

            [T.TextPrimary] = "#ECEFF4",
            [T.TextSecondary] = "#D8DEE9",
            [T.TextMuted] = "#9BA5B7",
            [T.TextDisabled] = "#767F91",
            [T.TextLink] = "#88C0D0",
            [T.TextPlaceholder] = "#9BA5B7",

            [T.BorderDefault] = "#434C5E",
            [T.BorderSubtle] = "#3B4252",
            [T.BorderFocus] = "#88C0D0",
            [T.BorderActive] = "#88C0D0",

            // Frost #5E81AC only reaches 4.0:1 against white, so the accent is one step darker.
            [T.AccentBackground] = "#4C6E99",
            [T.AccentHoverBackground] = "#5E81AC",
            [T.AccentPressedBackground] = "#405D82",
            [T.AccentMuted] = "#3B4A5E",

            [T.ListBackground] = "#2E3440",
            [T.ListHoverBackground] = "#363D4C",
            [T.ListSelectedBackground] = "#3F5C85",
            [T.ListSelectedForeground] = "#ECEFF4",
            [T.ListSelectedHoverBackground] = "#4A6B99",
            [T.ListSelectedInactiveBackground] = "#3B4252",
            [T.ListSelectedInactiveForeground] = "#D8DEE9",
            [T.ListDropBackground] = "#35506E",
            [T.ListDropBorder] = "#88C0D0",
            [T.ListHeaderBackground] = "#272C36",
            [T.ListHeaderForeground] = "#D8DEE9",
            [T.ListHeaderHoverBackground] = "#363D4C",
            [T.ListHeaderBorder] = "#434C5E",

            [T.TreeChevronForeground] = "#D8DEE9",
            [T.TreeChevronHoverForeground] = "#88C0D0",

            [T.TabStripBackground] = "#272C36",
            [T.TabActiveBackground] = "#2E3440",
            [T.TabActiveForeground] = "#ECEFF4",
            [T.TabInactiveBackground] = "#333945",
            [T.TabInactiveForeground] = "#9BA5B7",
            [T.TabHoverBackground] = "#3B4252",
            [T.TabBorder] = "#272C36",
            [T.TabActiveIndicator] = "#88C0D0",

            [T.TitleBarBackground] = "#272C36",
            [T.TitleBarInactiveBackground] = "#272C36",
            [T.TitleBarForeground] = "#ECEFF4",
            [T.TitleBarInactiveForeground] = "#9BA5B7",
            [T.TitleBarButtonHoverBackground] = "#3B4252",
            [T.TitleBarButtonPressedBackground] = "#434C5E",
            [T.TitleBarCloseHoverBackground] = "#BF616A",
            [T.TitleBarBorder] = "#1C2029",

            [T.ToolbarBackground] = "#3B4252",
            [T.ToolbarForeground] = "#ECEFF4",
            [T.ToolbarHoverBackground] = "#4C566A",
            [T.ToolbarPressedBackground] = "#566072",
            [T.ToolbarCheckedForeground] = "#8FBCBB",

            [T.InputBackground] = "#272C36",
            [T.InputForeground] = "#ECEFF4",
            [T.InputBorder] = "#434C5E",
            [T.InputSelectionBackground] = "#3F5C85",
            [T.InputSelectionForeground] = "#ECEFF4",
            [T.InputCaret] = "#ECEFF4",

            [T.ButtonSecondaryBackground] = "#3B4252",
            [T.ButtonSecondaryHoverBackground] = "#434C5E",
            [T.ButtonSecondaryPressedBackground] = "#4C566A",
            [T.ButtonSecondaryForeground] = "#ECEFF4",
            [T.ButtonDisabledBackground] = "#333945",

            [T.MenuBackground] = "#272C36",
            [T.MenuForeground] = "#ECEFF4",
            [T.MenuBorder] = "#4C566A",
            [T.MenuHoverBackground] = "#4C6E99",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#3B4252",
            [T.MenuIconForeground] = "#D8DEE9",
            [T.MenuGestureForeground] = "#9BA5B7",

            [T.ScrollBarThumb] = "#616E88AA",
            [T.ScrollBarThumbHover] = "#7B88A3CC",
            [T.ScrollBarThumbActive] = "#9BA5B7",

            [T.StatusBarBackground] = "#4C6E99",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#BF616A",
            [T.ErrorBackground] = "#3E2A2E",
            [T.ErrorBorder] = "#BF616A",
            [T.WarningForeground] = "#EBCB8B",
            [T.WarningBackground] = "#3A3325",
            [T.WarningBorder] = "#D0AE68",
            [T.ProgressIndicator] = "#88C0D0",
            [T.ProgressTrack] = "#3B4252",

            [T.SliderTrack] = "#434C5E",
            [T.SliderThumb] = "#88C0D0",
            [T.SliderThumbHover] = "#A8D8E6",
            [T.CheckBoxBackground] = "#272C36",
            [T.CheckBoxBorder] = "#4C566A",
            [T.CheckBoxCheckedBackground] = "#4C6E99",
            [T.GroupBoxHeaderForeground] = "#ECEFF4",

            [T.SplitterHoverBackground] = "#88C0D0",
            [T.MarqueeFill] = "#3388C0D0",
            [T.MarqueeBorder] = "#88C0D0",
            [T.ThumbnailTileBackground] = "#3B4252",
        },
    };

    public static ThemeDefinition GruvboxDark { get; } = new()
    {
        Id = "gruvbox-dark",
        Name = "Gruvbox Dark",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#282828",
            [T.SurfaceBackground] = "#1D2021",
            [T.SurfaceRaised] = "#3C3836",
            [T.OverlayBackground] = "#1D2021",
            [T.OverlayBorder] = "#665C54",

            [T.TextPrimary] = "#EBDBB2",
            [T.TextSecondary] = "#D5C4A1",
            [T.TextMuted] = "#A89984",
            [T.TextDisabled] = "#7C6F64",
            [T.TextLink] = "#83A598",
            [T.TextPlaceholder] = "#A89984",

            [T.BorderDefault] = "#504945",
            [T.BorderSubtle] = "#3C3836",
            [T.BorderFocus] = "#FE8019",
            [T.BorderActive] = "#FE8019",

            [T.AccentBackground] = "#AF3A03",
            [T.AccentHoverBackground] = "#C64E13",
            [T.AccentPressedBackground] = "#8F2F02",
            [T.AccentMuted] = "#4A2A16",

            [T.ListBackground] = "#282828",
            [T.ListHoverBackground] = "#32302F",
            [T.ListSelectedBackground] = "#504945",
            [T.ListSelectedForeground] = "#FBF1C7",
            [T.ListSelectedHoverBackground] = "#5E554F",
            [T.ListSelectedInactiveBackground] = "#3C3836",
            [T.ListSelectedInactiveForeground] = "#EBDBB2",
            [T.ListDropBackground] = "#4A3A22",
            [T.ListDropBorder] = "#FE8019",
            [T.ListHeaderBackground] = "#1D2021",
            [T.ListHeaderForeground] = "#D5C4A1",
            [T.ListHeaderHoverBackground] = "#32302F",
            [T.ListHeaderBorder] = "#504945",

            [T.TreeChevronForeground] = "#D5C4A1",
            [T.TreeChevronHoverForeground] = "#FE8019",

            [T.TabStripBackground] = "#1D2021",
            [T.TabActiveBackground] = "#282828",
            [T.TabActiveForeground] = "#FBF1C7",
            [T.TabInactiveBackground] = "#32302F",
            [T.TabInactiveForeground] = "#A89984",
            [T.TabHoverBackground] = "#3C3836",
            [T.TabBorder] = "#1D2021",
            [T.TabActiveIndicator] = "#FE8019",

            [T.TitleBarBackground] = "#1D2021",
            [T.TitleBarInactiveBackground] = "#1D2021",
            [T.TitleBarForeground] = "#EBDBB2",
            [T.TitleBarInactiveForeground] = "#A89984",
            [T.TitleBarButtonHoverBackground] = "#3C3836",
            [T.TitleBarButtonPressedBackground] = "#504945",
            [T.TitleBarCloseHoverBackground] = "#CC241D",
            [T.TitleBarBorder] = "#141617",

            [T.ToolbarBackground] = "#3C3836",
            [T.ToolbarForeground] = "#EBDBB2",
            [T.ToolbarHoverBackground] = "#504945",
            [T.ToolbarPressedBackground] = "#665C54",
            [T.ToolbarCheckedForeground] = "#FABD2F",

            [T.InputBackground] = "#1D2021",
            [T.InputForeground] = "#EBDBB2",
            [T.InputBorder] = "#504945",
            [T.InputSelectionBackground] = "#504945",
            [T.InputSelectionForeground] = "#FBF1C7",
            [T.InputCaret] = "#EBDBB2",

            [T.ButtonSecondaryBackground] = "#3C3836",
            [T.ButtonSecondaryHoverBackground] = "#504945",
            [T.ButtonSecondaryPressedBackground] = "#665C54",
            [T.ButtonSecondaryForeground] = "#EBDBB2",
            [T.ButtonDisabledBackground] = "#32302F",

            [T.MenuBackground] = "#1D2021",
            [T.MenuForeground] = "#EBDBB2",
            [T.MenuBorder] = "#665C54",
            [T.MenuHoverBackground] = "#AF3A03",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#3C3836",
            [T.MenuIconForeground] = "#D5C4A1",
            [T.MenuGestureForeground] = "#A89984",

            [T.ScrollBarThumb] = "#928374AA",
            [T.ScrollBarThumbHover] = "#A89984CC",
            [T.ScrollBarThumbActive] = "#D5C4A1",

            [T.StatusBarBackground] = "#AF3A03",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#FB4934",
            [T.ErrorBackground] = "#422322",
            [T.ErrorBorder] = "#CC241D",
            [T.WarningForeground] = "#FABD2F",
            [T.WarningBackground] = "#40331A",
            [T.WarningBorder] = "#D79921",
            [T.ProgressIndicator] = "#FE8019",
            [T.ProgressTrack] = "#3C3836",

            [T.SliderTrack] = "#504945",
            [T.SliderThumb] = "#FE8019",
            [T.SliderThumbHover] = "#FF9642",
            [T.CheckBoxBackground] = "#1D2021",
            [T.CheckBoxBorder] = "#665C54",
            [T.CheckBoxCheckedBackground] = "#AF3A03",
            [T.GroupBoxHeaderForeground] = "#EBDBB2",

            [T.SplitterHoverBackground] = "#FE8019",
            [T.MarqueeFill] = "#33FE8019",
            [T.MarqueeBorder] = "#FE8019",
            [T.ThumbnailTileBackground] = "#3C3836",
        },
    };

    public static ThemeDefinition Synthwave { get; } = new()
    {
        Id = "synthwave",
        Name = "Synthwave",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#1E1A2E",
            [T.SurfaceBackground] = "#171327",
            [T.SurfaceRaised] = "#2A2440",
            [T.OverlayBackground] = "#171327",
            [T.OverlayBorder] = "#4B3D7A",
            [T.ShadowColor] = "#B3000000",

            [T.TextPrimary] = "#EDE7FF",
            [T.TextSecondary] = "#C6B8F0",
            [T.TextMuted] = "#9B8CC9",
            [T.TextDisabled] = "#6F63A0",
            [T.TextLink] = "#36F9F6",
            [T.TextPlaceholder] = "#9B8CC9",

            [T.BorderDefault] = "#3A3159",
            [T.BorderSubtle] = "#2A2440",
            [T.BorderFocus] = "#FF7EDB",
            [T.BorderActive] = "#FF7EDB",

            [T.AccentBackground] = "#A32C86",
            [T.AccentHoverBackground] = "#BE3A9E",
            [T.AccentPressedBackground] = "#85206D",
            [T.AccentMuted] = "#3D1E38",

            [T.ListBackground] = "#1E1A2E",
            [T.ListHoverBackground] = "#272141",
            [T.ListSelectedBackground] = "#472F73",
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = "#56398A",
            [T.ListSelectedInactiveBackground] = "#2A2440",
            [T.ListSelectedInactiveForeground] = "#EDE7FF",
            [T.ListDropBackground] = "#3B1F55",
            [T.ListDropBorder] = "#FF7EDB",
            [T.ListHeaderBackground] = "#171327",
            [T.ListHeaderForeground] = "#C6B8F0",
            [T.ListHeaderHoverBackground] = "#272141",
            [T.ListHeaderBorder] = "#3A3159",

            [T.TreeChevronForeground] = "#C6B8F0",
            [T.TreeChevronHoverForeground] = "#36F9F6",

            [T.TabStripBackground] = "#171327",
            [T.TabActiveBackground] = "#1E1A2E",
            [T.TabActiveForeground] = "#FFFFFF",
            [T.TabInactiveBackground] = "#241F3A",
            [T.TabInactiveForeground] = "#9B8CC9",
            [T.TabHoverBackground] = "#2A2440",
            [T.TabBorder] = "#171327",
            [T.TabActiveIndicator] = "#FF7EDB",

            [T.TitleBarBackground] = "#171327",
            [T.TitleBarInactiveBackground] = "#171327",
            [T.TitleBarForeground] = "#EDE7FF",
            [T.TitleBarInactiveForeground] = "#9B8CC9",
            [T.TitleBarButtonHoverBackground] = "#2A2440",
            [T.TitleBarButtonPressedBackground] = "#3A3159",
            [T.TitleBarCloseHoverBackground] = "#FE4450",
            [T.TitleBarBorder] = "#0E0B1A",

            [T.ToolbarBackground] = "#2A2440",
            [T.ToolbarForeground] = "#EDE7FF",
            [T.ToolbarHoverBackground] = "#3A3159",
            [T.ToolbarPressedBackground] = "#473C6E",
            [T.ToolbarCheckedForeground] = "#36F9F6",

            [T.InputBackground] = "#171327",
            [T.InputForeground] = "#EDE7FF",
            [T.InputBorder] = "#3A3159",
            [T.InputSelectionBackground] = "#472F73",
            [T.InputCaret] = "#FF7EDB",

            [T.ButtonSecondaryBackground] = "#2A2440",
            [T.ButtonSecondaryHoverBackground] = "#3A3159",
            [T.ButtonSecondaryPressedBackground] = "#473C6E",
            [T.ButtonSecondaryForeground] = "#EDE7FF",
            [T.ButtonDisabledBackground] = "#241F3A",

            [T.MenuBackground] = "#171327",
            [T.MenuForeground] = "#EDE7FF",
            [T.MenuBorder] = "#4B3D7A",
            [T.MenuHoverBackground] = "#A32C86",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#3A3159",
            [T.MenuIconForeground] = "#C6B8F0",
            [T.MenuGestureForeground] = "#9B8CC9",

            [T.ScrollBarThumb] = "#9B8CC999",
            [T.ScrollBarThumbHover] = "#C6B8F0CC",
            [T.ScrollBarThumbActive] = "#FF7EDB",

            [T.StatusBarBackground] = "#A32C86",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#FE4450",
            [T.ErrorBackground] = "#3F1B28",
            [T.ErrorBorder] = "#FE4450",
            [T.WarningForeground] = "#FEDE5D",
            [T.WarningBackground] = "#3A3320",
            [T.WarningBorder] = "#D4B840",
            [T.ProgressIndicator] = "#FF7EDB",
            [T.ProgressTrack] = "#2A2440",

            [T.SliderTrack] = "#3A3159",
            [T.SliderThumb] = "#FF7EDB",
            [T.SliderThumbHover] = "#FFA8E8",
            [T.CheckBoxBackground] = "#171327",
            [T.CheckBoxBorder] = "#4B3D7A",
            [T.CheckBoxCheckedBackground] = "#A32C86",
            [T.GroupBoxHeaderForeground] = "#EDE7FF",

            [T.SplitterHoverBackground] = "#FF7EDB",
            [T.MarqueeFill] = "#33FF7EDB",
            [T.MarqueeBorder] = "#FF7EDB",
            [T.ThumbnailTileBackground] = "#2A2440",
        },
    };

    public static ThemeDefinition CatppuccinLatte { get; } = new()
    {
        Id = "catppuccin-latte",
        Name = "Catppuccin Latte",
        BaseThemeId = "light-plus",
        IsDark = false,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#EFF1F5",
            [T.SurfaceBackground] = "#E6E9EF",
            [T.SurfaceRaised] = "#DCE0E8",
            [T.OverlayBackground] = "#FFFFFF",
            [T.OverlayBorder] = "#BCC0CC",

            [T.TextPrimary] = "#4C4F69",
            [T.TextSecondary] = "#5C5F77",
            [T.TextMuted] = "#6C6F85",
            [T.TextDisabled] = "#9CA0B0",
            [T.TextLink] = "#1E66F5",
            [T.TextPlaceholder] = "#7C7F93",

            [T.BorderDefault] = "#BCC0CC",
            [T.BorderSubtle] = "#DCE0E8",
            [T.BorderFocus] = "#1E66F5",
            [T.BorderActive] = "#8839EF",

            [T.AccentBackground] = "#8839EF",
            [T.AccentHoverBackground] = "#9A52F5",
            [T.AccentPressedBackground] = "#7229D0",
            [T.AccentMuted] = "#E5D5FB",

            [T.ListBackground] = "#FFFFFF",
            [T.ListHoverBackground] = "#E6E9EF",
            [T.ListSelectedBackground] = "#8839EF",
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = "#7B2DE0",
            [T.ListSelectedInactiveBackground] = "#DDD6F3",
            [T.ListSelectedInactiveForeground] = "#4C4F69",
            [T.ListDropBackground] = "#E5D5FB",
            [T.ListDropBorder] = "#8839EF",
            [T.ListHeaderBackground] = "#E6E9EF",
            [T.ListHeaderForeground] = "#5C5F77",
            [T.ListHeaderHoverBackground] = "#DCE0E8",
            [T.ListHeaderBorder] = "#CCD0DA",

            [T.TreeChevronForeground] = "#6C6F85",
            [T.TreeChevronHoverForeground] = "#8839EF",

            [T.TabStripBackground] = "#E6E9EF",
            [T.TabActiveBackground] = "#FFFFFF",
            [T.TabActiveForeground] = "#4C4F69",
            [T.TabInactiveBackground] = "#DCE0E8",
            [T.TabInactiveForeground] = "#6C6F85",
            [T.TabHoverBackground] = "#D3D8E2",
            [T.TabBorder] = "#CCD0DA",
            [T.TabActiveIndicator] = "#8839EF",

            [T.TitleBarBackground] = "#DCE0E8",
            [T.TitleBarInactiveBackground] = "#E6E9EF",
            [T.TitleBarForeground] = "#4C4F69",
            [T.TitleBarInactiveForeground] = "#7C7F93",
            [T.TitleBarButtonHoverBackground] = "#CCD0DA",
            [T.TitleBarButtonPressedBackground] = "#BCC0CC",
            [T.TitleBarCloseHoverBackground] = "#D20F39",
            [T.TitleBarBorder] = "#BCC0CC",

            [T.ToolbarBackground] = "#E6E9EF",
            [T.ToolbarForeground] = "#4C4F69",
            [T.ToolbarHoverBackground] = "#D3D8E2",
            [T.ToolbarPressedBackground] = "#C4C9D6",
            [T.ToolbarCheckedForeground] = "#8839EF",

            [T.InputBackground] = "#FFFFFF",
            [T.InputForeground] = "#4C4F69",
            [T.InputBorder] = "#BCC0CC",
            [T.InputSelectionBackground] = "#D8C4F9",
            [T.InputSelectionForeground] = "#4C4F69",
            [T.InputCaret] = "#8839EF",

            [T.ButtonSecondaryBackground] = "#DCE0E8",
            [T.ButtonSecondaryHoverBackground] = "#CCD0DA",
            [T.ButtonSecondaryPressedBackground] = "#BCC0CC",
            [T.ButtonSecondaryForeground] = "#4C4F69",
            [T.ButtonDisabledBackground] = "#E6E9EF",
            [T.ButtonDisabledForeground] = "#9CA0B0",

            [T.MenuBackground] = "#FFFFFF",
            [T.MenuForeground] = "#4C4F69",
            [T.MenuBorder] = "#BCC0CC",
            [T.MenuHoverBackground] = "#8839EF",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#DCE0E8",
            [T.MenuIconForeground] = "#6C6F85",
            [T.MenuGestureForeground] = "#7C7F93",
            [T.MenuDisabledForeground] = "#9CA0B0",

            [T.ScrollBarThumb] = "#7C7F9380",
            [T.ScrollBarThumbHover] = "#6C6F85B3",
            [T.ScrollBarThumbActive] = "#4C4F69CC",

            [T.StatusBarBackground] = "#8839EF",
            [T.StatusBarForeground] = "#FFFFFF",
            [T.StatusBarHoverBackground] = "#FFFFFF33",

            [T.ErrorForeground] = "#D20F39",
            [T.ErrorBackground] = "#FBE4E8",
            [T.ErrorBorder] = "#D20F39",
            [T.WarningForeground] = "#A56A00",
            [T.WarningBackground] = "#FBF1DE",
            [T.WarningBorder] = "#DF8E1D",
            [T.ProgressIndicator] = "#8839EF",
            [T.ProgressTrack] = "#DCE0E8",

            [T.SliderTrack] = "#BCC0CC",
            [T.SliderThumb] = "#8839EF",
            [T.SliderThumbHover] = "#7229D0",
            [T.CheckBoxBackground] = "#FFFFFF",
            [T.CheckBoxBorder] = "#8C8FA1",
            [T.CheckBoxCheckedBackground] = "#8839EF",
            [T.GroupBoxHeaderForeground] = "#4C4F69",

            [T.SplitterHoverBackground] = "#8839EF",
            [T.MarqueeFill] = "#338839EF",
            [T.MarqueeBorder] = "#8839EF",
            [T.ThumbnailTileBackground] = "#00000000",
        },
    };

    public static ThemeDefinition HighContrastDark { get; } = new()
    {
        Id = "high-contrast-dark",
        Name = "High Contrast Dark",
        BaseThemeId = "dark-plus",
        IsDark = true,
        IsBuiltIn = true,
        Colors = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [T.WindowBackground] = "#000000",
            [T.SurfaceBackground] = "#000000",
            [T.SurfaceRaised] = "#000000",
            [T.OverlayBackground] = "#000000",
            [T.OverlayBorder] = "#6FC3DF",

            [T.TextPrimary] = "#FFFFFF",
            [T.TextSecondary] = "#DFDFDF",
            [T.TextMuted] = "#CFCFCF",
            [T.TextDisabled] = "#A0A0A0",
            [T.TextLink] = "#6FC3DF",
            [T.TextPlaceholder] = "#DFDFDF",

            [T.BorderDefault] = "#6FC3DF",
            [T.BorderSubtle] = "#6FC3DF",
            [T.BorderFocus] = "#F38518",
            [T.BorderActive] = "#F38518",

            [T.AccentBackground] = "#0F4A85",
            [T.AccentHoverBackground] = "#1A5FA5",
            [T.AccentPressedBackground] = "#0B3A69",
            [T.AccentMuted] = "#0F4A85",

            [T.ListBackground] = "#000000",
            [T.ListHoverBackground] = "#2E2E2E",
            [T.ListSelectedBackground] = "#0F4A85",
            [T.ListSelectedForeground] = "#FFFFFF",
            [T.ListSelectedHoverBackground] = "#1A5FA5",
            [T.ListSelectedInactiveBackground] = "#2E2E2E",
            [T.ListSelectedInactiveForeground] = "#FFFFFF",
            [T.ListDropBackground] = "#0F4A85",
            [T.ListDropBorder] = "#F38518",
            [T.ListHeaderBackground] = "#000000",
            [T.ListHeaderForeground] = "#FFFFFF",
            [T.ListHeaderHoverBackground] = "#2E2E2E",
            [T.ListHeaderBorder] = "#6FC3DF",

            [T.TreeChevronForeground] = "#FFFFFF",
            [T.TreeChevronHoverForeground] = "#F38518",

            [T.TabStripBackground] = "#000000",
            [T.TabActiveBackground] = "#000000",
            [T.TabActiveForeground] = "#FFFFFF",
            [T.TabInactiveBackground] = "#000000",
            [T.TabInactiveForeground] = "#CFCFCF",
            [T.TabHoverBackground] = "#2E2E2E",
            [T.TabBorder] = "#6FC3DF",
            [T.TabActiveIndicator] = "#F38518",

            [T.TitleBarBackground] = "#000000",
            [T.TitleBarInactiveBackground] = "#000000",
            [T.TitleBarForeground] = "#FFFFFF",
            [T.TitleBarInactiveForeground] = "#CFCFCF",
            [T.TitleBarButtonHoverBackground] = "#2E2E2E",
            [T.TitleBarButtonPressedBackground] = "#4A4A4A",
            [T.TitleBarBorder] = "#6FC3DF",

            [T.ToolbarBackground] = "#000000",
            [T.ToolbarForeground] = "#FFFFFF",
            [T.ToolbarHoverBackground] = "#2E2E2E",
            [T.ToolbarPressedBackground] = "#4A4A4A",
            [T.ToolbarCheckedForeground] = "#F38518",

            [T.InputBackground] = "#000000",
            [T.InputForeground] = "#FFFFFF",
            [T.InputBorder] = "#6FC3DF",
            [T.InputSelectionBackground] = "#0F4A85",
            [T.InputCaret] = "#FFFFFF",

            [T.ButtonSecondaryBackground] = "#000000",
            [T.ButtonSecondaryHoverBackground] = "#2E2E2E",
            [T.ButtonSecondaryPressedBackground] = "#4A4A4A",
            [T.ButtonSecondaryForeground] = "#FFFFFF",
            [T.ButtonDisabledBackground] = "#000000",
            [T.ButtonDisabledForeground] = "#A0A0A0",

            [T.MenuBackground] = "#000000",
            [T.MenuForeground] = "#FFFFFF",
            [T.MenuBorder] = "#6FC3DF",
            [T.MenuHoverBackground] = "#0F4A85",
            [T.MenuHoverForeground] = "#FFFFFF",
            [T.MenuSeparator] = "#6FC3DF",
            [T.MenuIconForeground] = "#FFFFFF",
            [T.MenuGestureForeground] = "#CFCFCF",

            [T.ScrollBarThumb] = "#6FC3DF99",
            [T.ScrollBarThumbHover] = "#6FC3DFCC",
            [T.ScrollBarThumbActive] = "#6FC3DF",

            [T.StatusBarBackground] = "#0F4A85",
            [T.StatusBarForeground] = "#FFFFFF",

            [T.ErrorForeground] = "#F48771",
            [T.WarningForeground] = "#FFD700",
            [T.ProgressIndicator] = "#6FC3DF",
            [T.ProgressTrack] = "#000000",

            [T.SliderTrack] = "#6FC3DF",
            [T.SliderThumb] = "#FFFFFF",
            [T.SliderThumbHover] = "#F38518",
            [T.CheckBoxBackground] = "#000000",
            [T.CheckBoxBorder] = "#6FC3DF",
            [T.CheckBoxCheckedBackground] = "#0F4A85",

            [T.SplitterHoverBackground] = "#F38518",
            [T.MarqueeFill] = "#33F38518",
            [T.MarqueeBorder] = "#F38518",
            [T.ThumbnailTileBackground] = "#2E2E2E",
        },
    };

    /// <summary>The themes that stand alone: these define every token directly, with no base.</summary>
    public static IReadOnlyList<ThemeDefinition> Roots { get; } = new[] { DarkPlus, LightPlus };

    /// <summary>
    /// Picker order: the two roots first, then the dark sheets, then the light one, with the
    /// accessibility theme last so it doesn't read as just another colour scheme.
    /// </summary>
    public static IReadOnlyList<ThemeDefinition> BuiltIns { get; } = new[]
    {
        DarkPlus,
        LightPlus,
        TokyoNight,
        CatppuccinMocha,
        Dracula,
        Nord,
        GruvboxDark,
        Synthwave,
        Monokai,
        SolarizedDark,
        CatppuccinLatte,
        HighContrastDark,
    };

    /// <summary>What the app falls back to when the selected theme cannot be loaded.</summary>
    public static ThemeDefinition Default => DarkPlus;

    private static readonly Dictionary<string, ThemeDefinition> ById =
        BuiltIns.ToDictionary(t => t.Id, StringComparer.OrdinalIgnoreCase);

    public static ThemeDefinition? Find(string id) =>
        ById.TryGetValue(id, out var theme) ? theme : null;
}
