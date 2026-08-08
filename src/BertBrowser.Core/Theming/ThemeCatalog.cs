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

    public static IReadOnlyList<ThemeDefinition> BuiltIns { get; } =
        new[] { DarkPlus, LightPlus, Monokai, SolarizedDark, HighContrastDark };

    /// <summary>What the app falls back to when the selected theme cannot be loaded.</summary>
    public static ThemeDefinition Default => DarkPlus;

    private static readonly Dictionary<string, ThemeDefinition> ById =
        BuiltIns.ToDictionary(t => t.Id, StringComparer.OrdinalIgnoreCase);

    public static ThemeDefinition? Find(string id) =>
        ById.TryGetValue(id, out var theme) ? theme : null;
}
