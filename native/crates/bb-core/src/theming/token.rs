//! Every colour the app can be themed with. A token's name is used verbatim as the JSON property
//! name in a theme file and as the identity of a row in the theme editor (the C# also used it as the
//! WPF resource key). The descriptor list is the single source of truth: a token cannot be themed
//! without also being describable in the editor, and the UI indexes a palette array by the token's
//! discriminant, which is why the enum is `repr(u16)` and `ALL` is in declaration order.
//! Spec: src/BertBrowser.Core/Theming/ThemeToken.cs, ThemeTokenDescriptor.cs.

/// Editor-facing metadata for one colour token. `key` is the only identity that matters: it is
/// simultaneously the JSON property name and what the theme editor writes back, so there is exactly
/// one name for a colour across the whole app. `is_core` marks the handful worth showing before the
/// user asks for "all colours"; everything else is still editable, just not in the default list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThemeTokenDescriptor {
    pub token: ThemeToken,
    pub key: &'static str,
    pub group: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub is_core: bool,
}

const GROUP_SURFACES: &str = "Surfaces";
const GROUP_TEXT: &str = "Text";
const GROUP_BORDERS: &str = "Borders";
const GROUP_ACCENT: &str = "Accent";
const GROUP_LISTS: &str = "Lists";
const GROUP_COMPARE: &str = "Folder compare";
const GROUP_CHANGES: &str = "Change timeline";
const GROUP_TREE: &str = "Folder tree";
const GROUP_TABS: &str = "Tabs";
const GROUP_TITLE_BAR: &str = "Title bar";
const GROUP_TOOLBAR: &str = "Toolbar";
const GROUP_INPUTS: &str = "Inputs";
const GROUP_BUTTONS: &str = "Buttons";
const GROUP_MENUS: &str = "Menus";
const GROUP_SCROLL_BARS: &str = "Scrollbars";
const GROUP_STATUS_BAR: &str = "Status bar";
const GROUP_FEEDBACK: &str = "Feedback";
const GROUP_CONTROLS: &str = "Controls";
const GROUP_SYNTAX: &str = "Code preview";
const GROUP_MISC: &str = "Misc";

macro_rules! theme_tokens {
    ($( $variant:ident = $key:literal, $group:ident, $display:literal, $desc:literal, $core:literal; )*) => {
        /// One colour slot. The discriminant is the index into a resolved palette.
        #[repr(u16)]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum ThemeToken {
            $( $variant, )*
        }

        impl ThemeToken {
            /// How many tokens there are: the size of a palette.
            pub const COUNT: usize = [$( $key, )*].len();

            /// Every token, in editor display order: groups appear in the order they first occur.
            pub const ALL: [ThemeToken; Self::COUNT] = [
                $( ThemeToken::$variant, )*
            ];

            /// The descriptors, in `ALL` order.
            pub const DESCRIPTORS: [ThemeTokenDescriptor; Self::COUNT] = [
                $( ThemeTokenDescriptor {
                    token: ThemeToken::$variant,
                    key: $key,
                    group: $group,
                    display_name: $display,
                    description: $desc,
                    is_core: $core,
                }, )*
            ];

            /// The dotted key, e.g. `Theme.Window.Background`.
            pub const fn name(self) -> &'static str {
                match self {
                    $( ThemeToken::$variant => $key, )*
                }
            }

            /// The token with this exact (case-sensitive) key, if any.
            pub fn from_name(key: &str) -> Option<ThemeToken> {
                match key {
                    $( $key => Some(ThemeToken::$variant), )*
                    _ => None,
                }
            }
        }
    };
}

theme_tokens! {
    // Surfaces
    WindowBackground = "Theme.Window.Background", GROUP_SURFACES, "Window background", "The file list and the window body.", true;
    SurfaceBackground = "Theme.Surface.Background", GROUP_SURFACES, "Panel background", "Sidebar, tab strip and dialog bodies.", true;
    SurfaceRaised = "Theme.Surface.Raised", GROUP_SURFACES, "Raised background", "Toolbar strip.", false;
    OverlayBackground = "Theme.Overlay.Background", GROUP_SURFACES, "Popup background", "Tooltips and dropdowns.", false;
    OverlayBorder = "Theme.Overlay.Border", GROUP_SURFACES, "Popup border", "", false;
    ShadowColor = "Theme.Shadow.Color", GROUP_SURFACES, "Shadow", "Drop shadow under popups and the pinned tree row; alpha is the shadow strength.", false;
    Scrim = "Theme.Scrim.Background", GROUP_SURFACES, "Backdrop dim", "Laid over the folders behind the settings page; alpha is how much it dims.", false;

    // Text
    TextPrimary = "Theme.Text.Primary", GROUP_TEXT, "Text", "Default foreground.", true;
    TextSecondary = "Theme.Text.Secondary", GROUP_TEXT, "Secondary text", "Sizes, dates, relative folders.", true;
    TextMuted = "Theme.Text.Muted", GROUP_TEXT, "Muted text", "Section headers and hints.", false;
    TextDisabled = "Theme.Text.Disabled", GROUP_TEXT, "Disabled text", "", false;
    TextOnAccent = "Theme.Text.OnAccent", GROUP_TEXT, "Text on accent", "", false;
    TextLink = "Theme.Text.Link", GROUP_TEXT, "Link", "The undo link in the status bar.", false;
    TextPlaceholder = "Theme.Text.Placeholder", GROUP_TEXT, "Placeholder", "The search box watermark.", false;

    // Borders
    BorderDefault = "Theme.Border.Default", GROUP_BORDERS, "Border", "", true;
    BorderSubtle = "Theme.Border.Subtle", GROUP_BORDERS, "Subtle border", "Separators and hairlines.", false;
    BorderFocus = "Theme.Border.Focus", GROUP_BORDERS, "Focus border", "The focused text field or control.", true;
    BorderActive = "Theme.Border.Active", GROUP_BORDERS, "Active pane border", "Outlines the pane the next keystroke lands in.", false;

    // Accent
    AccentBackground = "Theme.Accent.Background", GROUP_ACCENT, "Accent", "Primary buttons.", true;
    AccentHoverBackground = "Theme.Accent.HoverBackground", GROUP_ACCENT, "Accent hover", "", false;
    AccentPressedBackground = "Theme.Accent.PressedBackground", GROUP_ACCENT, "Accent pressed", "", false;
    AccentForeground = "Theme.Accent.Foreground", GROUP_ACCENT, "Accent foreground", "", true;
    AccentMuted = "Theme.Accent.Muted", GROUP_ACCENT, "Muted accent", "", false;

    // Lists (file list, folder tree, bookmarks, thumbnails)
    ListBackground = "Theme.List.Background", GROUP_LISTS, "List background", "", false;
    ListHoverBackground = "Theme.List.HoverBackground", GROUP_LISTS, "Row hover", "", true;
    ListSelectedBackground = "Theme.List.SelectedBackground", GROUP_LISTS, "Row selected", "", true;
    ListSelectedForeground = "Theme.List.SelectedForeground", GROUP_LISTS, "Row selected text", "", true;
    ListSelectedHoverBackground = "Theme.List.SelectedHoverBackground", GROUP_LISTS, "Row selected + hover", "", false;
    ListSelectedInactiveBackground = "Theme.List.SelectedInactiveBackground", GROUP_LISTS, "Row selected (unfocused)", "", false;
    ListSelectedInactiveForeground = "Theme.List.SelectedInactiveForeground", GROUP_LISTS, "Row selected text (unfocused)", "", false;
    ListDropBackground = "Theme.List.DropBackground", GROUP_LISTS, "Drop target", "The folder a drag is hovering.", false;
    ListDropBorder = "Theme.List.DropBorder", GROUP_LISTS, "Drop target border", "", false;
    ListHeaderBackground = "Theme.List.HeaderBackground", GROUP_LISTS, "Column header", "", false;
    ListHeaderForeground = "Theme.List.HeaderForeground", GROUP_LISTS, "Column header text", "", false;
    ListHeaderHoverBackground = "Theme.List.HeaderHoverBackground", GROUP_LISTS, "Column header hover", "", false;
    ListHeaderBorder = "Theme.List.HeaderBorder", GROUP_LISTS, "Column header border", "", false;

    // Row tints while two folders are being compared. Translucent on purpose, like MarqueeFill: they
    // composite over whatever list background a theme has, so nineteen built-ins do not each need
    // five hand-tuned colours to keep a row readable.
    CompareOnlyHere = "Theme.Compare.OnlyHere", GROUP_COMPARE, "Only on this side", "Alpha matters: these tint a row rather than paint it.", false;
    CompareNewer = "Theme.Compare.Newer", GROUP_COMPARE, "Newer here", "", false;
    CompareOlder = "Theme.Compare.Older", GROUP_COMPARE, "Older here", "", false;
    CompareDiffers = "Theme.Compare.Differs", GROUP_COMPARE, "Differs", "Both sides have it and neither is newer.", false;
    CompareUnknown = "Theme.Compare.Unknown", GROUP_COMPARE, "Could not be compared", "Never treated as a match, and never synced.", false;

    // Row tints in the change timeline, translucent for the reason the compare tints are.
    ChangeCreated = "Theme.Change.Created", GROUP_CHANGES, "Created", "Alpha matters: these tint a row in the \"What changed\" window rather than paint it.", false;
    ChangeModified = "Theme.Change.Modified", GROUP_CHANGES, "Modified", "", false;
    ChangeDeleted = "Theme.Change.Deleted", GROUP_CHANGES, "Deleted", "", false;
    ChangeRenamed = "Theme.Change.Renamed", GROUP_CHANGES, "Renamed", "", false;

    // Folder tree
    TreeChevronForeground = "Theme.Tree.ChevronForeground", GROUP_TREE, "Expander chevron", "", false;
    TreeChevronHoverForeground = "Theme.Tree.ChevronHoverForeground", GROUP_TREE, "Expander chevron hover", "", false;

    // Tab strip
    TabStripBackground = "Theme.Tab.StripBackground", GROUP_TABS, "Tab strip", "", false;
    TabActiveBackground = "Theme.Tab.ActiveBackground", GROUP_TABS, "Active tab", "", true;
    TabActiveForeground = "Theme.Tab.ActiveForeground", GROUP_TABS, "Active tab text", "", false;
    TabInactiveBackground = "Theme.Tab.InactiveBackground", GROUP_TABS, "Inactive tab", "", true;
    TabInactiveForeground = "Theme.Tab.InactiveForeground", GROUP_TABS, "Inactive tab text", "", false;
    TabHoverBackground = "Theme.Tab.HoverBackground", GROUP_TABS, "Tab hover", "", false;
    TabBorder = "Theme.Tab.Border", GROUP_TABS, "Tab border", "", false;
    TabActiveIndicator = "Theme.Tab.ActiveIndicator", GROUP_TABS, "Active tab indicator", "The stripe along the top of the active tab.", false;

    // Title bar
    TitleBarBackground = "Theme.TitleBar.Background", GROUP_TITLE_BAR, "Title bar", "", true;
    TitleBarInactiveBackground = "Theme.TitleBar.InactiveBackground", GROUP_TITLE_BAR, "Title bar (unfocused)", "", false;
    TitleBarForeground = "Theme.TitleBar.Foreground", GROUP_TITLE_BAR, "Title bar text", "", false;
    TitleBarInactiveForeground = "Theme.TitleBar.InactiveForeground", GROUP_TITLE_BAR, "Title bar text (unfocused)", "", false;
    TitleBarButtonHoverBackground = "Theme.TitleBar.ButtonHoverBackground", GROUP_TITLE_BAR, "Caption button hover", "", false;
    TitleBarButtonPressedBackground = "Theme.TitleBar.ButtonPressedBackground", GROUP_TITLE_BAR, "Caption button pressed", "", false;
    TitleBarCloseHoverBackground = "Theme.TitleBar.CloseHoverBackground", GROUP_TITLE_BAR, "Close button hover", "", false;
    TitleBarCloseHoverForeground = "Theme.TitleBar.CloseHoverForeground", GROUP_TITLE_BAR, "Close button hover glyph", "", false;
    TitleBarBorder = "Theme.TitleBar.Border", GROUP_TITLE_BAR, "Window border", "", false;

    // Toolbar
    ToolbarBackground = "Theme.Toolbar.Background", GROUP_TOOLBAR, "Toolbar", "", false;
    ToolbarForeground = "Theme.Toolbar.Foreground", GROUP_TOOLBAR, "Toolbar icons", "", false;
    ToolbarHoverBackground = "Theme.Toolbar.HoverBackground", GROUP_TOOLBAR, "Toolbar button hover", "", false;
    ToolbarPressedBackground = "Theme.Toolbar.PressedBackground", GROUP_TOOLBAR, "Toolbar button pressed", "", false;
    ToolbarCheckedForeground = "Theme.Toolbar.CheckedForeground", GROUP_TOOLBAR, "Toolbar toggle on", "The show-hidden-items toggle when active.", false;

    // Text inputs
    InputBackground = "Theme.Input.Background", GROUP_INPUTS, "Field background", "Address bar and search box.", false;
    InputForeground = "Theme.Input.Foreground", GROUP_INPUTS, "Field text", "", false;
    InputBorder = "Theme.Input.Border", GROUP_INPUTS, "Field border", "", false;
    InputSelectionBackground = "Theme.Input.SelectionBackground", GROUP_INPUTS, "Text selection", "", false;
    InputSelectionForeground = "Theme.Input.SelectionForeground", GROUP_INPUTS, "Selected text", "", false;
    InputCaret = "Theme.Input.Caret", GROUP_INPUTS, "Caret", "", false;

    // Buttons (primary buttons reuse the accent tokens)
    ButtonSecondaryBackground = "Theme.Button.SecondaryBackground", GROUP_BUTTONS, "Button", "", false;
    ButtonSecondaryHoverBackground = "Theme.Button.SecondaryHoverBackground", GROUP_BUTTONS, "Button hover", "", false;
    ButtonSecondaryPressedBackground = "Theme.Button.SecondaryPressedBackground", GROUP_BUTTONS, "Button pressed", "", false;
    ButtonSecondaryForeground = "Theme.Button.SecondaryForeground", GROUP_BUTTONS, "Button text", "", false;
    ButtonDisabledBackground = "Theme.Button.DisabledBackground", GROUP_BUTTONS, "Button disabled", "", false;
    ButtonDisabledForeground = "Theme.Button.DisabledForeground", GROUP_BUTTONS, "Button disabled text", "", false;

    // Context menus
    MenuBackground = "Theme.Menu.Background", GROUP_MENUS, "Menu background", "", false;
    MenuForeground = "Theme.Menu.Foreground", GROUP_MENUS, "Menu text", "", false;
    MenuBorder = "Theme.Menu.Border", GROUP_MENUS, "Menu border", "", false;
    MenuHoverBackground = "Theme.Menu.HoverBackground", GROUP_MENUS, "Menu item hover", "", false;
    MenuHoverForeground = "Theme.Menu.HoverForeground", GROUP_MENUS, "Menu item hover text", "", false;
    MenuSeparator = "Theme.Menu.Separator", GROUP_MENUS, "Menu separator", "", false;
    MenuIconForeground = "Theme.Menu.IconForeground", GROUP_MENUS, "Menu icon", "", false;
    MenuGestureForeground = "Theme.Menu.GestureForeground", GROUP_MENUS, "Menu shortcut text", "", false;
    MenuDisabledForeground = "Theme.Menu.DisabledForeground", GROUP_MENUS, "Menu disabled text", "", false;

    // Scrollbars
    ScrollBarBackground = "Theme.ScrollBar.Background", GROUP_SCROLL_BARS, "Scrollbar track", "Usually transparent, so the bar floats over content.", false;
    ScrollBarThumb = "Theme.ScrollBar.Thumb", GROUP_SCROLL_BARS, "Scrollbar thumb", "", true;
    ScrollBarThumbHover = "Theme.ScrollBar.ThumbHover", GROUP_SCROLL_BARS, "Scrollbar thumb hover", "", false;
    ScrollBarThumbActive = "Theme.ScrollBar.ThumbActive", GROUP_SCROLL_BARS, "Scrollbar thumb dragging", "", false;

    // Status bar
    StatusBarBackground = "Theme.StatusBar.Background", GROUP_STATUS_BAR, "Status bar", "", true;
    StatusBarForeground = "Theme.StatusBar.Foreground", GROUP_STATUS_BAR, "Status bar text", "", false;
    StatusBarMutedForeground = "Theme.StatusBar.MutedForeground", GROUP_STATUS_BAR, "Status bar secondary text", "", false;
    StatusBarHoverBackground = "Theme.StatusBar.HoverBackground", GROUP_STATUS_BAR, "Status bar item hover", "", false;

    // Feedback
    ErrorForeground = "Theme.Error.Foreground", GROUP_FEEDBACK, "Error text", "", false;
    ErrorBackground = "Theme.Error.Background", GROUP_FEEDBACK, "Error background", "", false;
    ErrorBorder = "Theme.Error.Border", GROUP_FEEDBACK, "Error border", "", false;
    WarningForeground = "Theme.Warning.Foreground", GROUP_FEEDBACK, "Warning text", "", false;
    WarningBackground = "Theme.Warning.Background", GROUP_FEEDBACK, "Warning background", "The banner above a folder that failed to list.", false;
    WarningBorder = "Theme.Warning.Border", GROUP_FEEDBACK, "Warning border", "", false;
    ProgressIndicator = "Theme.Progress.Indicator", GROUP_FEEDBACK, "Progress bar", "", false;
    ProgressTrack = "Theme.Progress.Track", GROUP_FEEDBACK, "Progress track", "", false;

    // Small controls
    SliderTrack = "Theme.Slider.Track", GROUP_CONTROLS, "Slider track", "", false;
    SliderThumb = "Theme.Slider.Thumb", GROUP_CONTROLS, "Slider thumb", "", false;
    SliderThumbHover = "Theme.Slider.ThumbHover", GROUP_CONTROLS, "Slider thumb hover", "", false;
    CheckBoxBackground = "Theme.CheckBox.Background", GROUP_CONTROLS, "Checkbox", "", false;
    CheckBoxBorder = "Theme.CheckBox.Border", GROUP_CONTROLS, "Checkbox border", "", false;
    CheckBoxGlyph = "Theme.CheckBox.Glyph", GROUP_CONTROLS, "Checkbox tick", "", false;
    CheckBoxCheckedBackground = "Theme.CheckBox.CheckedBackground", GROUP_CONTROLS, "Checkbox checked", "", false;
    GroupBoxHeaderForeground = "Theme.GroupBox.HeaderForeground", GROUP_CONTROLS, "Group header", "", false;

    // Everything else. Only the preview's checkerboard is here: the pane's chrome, text and borders
    // are the ordinary surface tokens, and a parallel set would be two places to change every time
    // the app's greys move.
    SplitterBackground = "Theme.Splitter.Background", GROUP_MISC, "Splitter", "", false;
    SplitterHoverBackground = "Theme.Splitter.HoverBackground", GROUP_MISC, "Splitter hover", "", false;
    MarqueeFill = "Theme.Marquee.Fill", GROUP_MISC, "Rubber band fill", "The drag-to-select rectangle; alpha matters here.", false;
    MarqueeBorder = "Theme.Marquee.Border", GROUP_MISC, "Rubber band border", "", false;
    ThumbnailTileBackground = "Theme.Thumbnail.TileBackground", GROUP_MISC, "Thumbnail tile", "Sits behind previews so transparent images still read.", false;
    PreviewCheckerLight = "Theme.Preview.CheckerLight", GROUP_MISC, "Preview checker (light)", "The chequerboard behind a transparent image in the preview pane.", false;
    PreviewCheckerDark = "Theme.Preview.CheckerDark", GROUP_MISC, "Preview checker (dark)", "The other square of the chequerboard.", false;

    // Syntax colouring for the text preview.
    SyntaxKeyword = "Theme.Syntax.Keyword", GROUP_SYNTAX, "Keyword", "Colouring in the preview pane's text view.", false;
    SyntaxString = "Theme.Syntax.String", GROUP_SYNTAX, "String", "", false;
    SyntaxComment = "Theme.Syntax.Comment", GROUP_SYNTAX, "Comment", "", false;
    SyntaxNumber = "Theme.Syntax.Number", GROUP_SYNTAX, "Number", "", false;
    SyntaxPunctuation = "Theme.Syntax.Punctuation", GROUP_SYNTAX, "Punctuation", "Brackets, operators and separators.", false;
}

impl ThemeToken {
    /// The palette index: the discriminant.
    pub const fn index(self) -> usize {
        self as u16 as usize
    }

    pub const fn describe(self) -> &'static ThemeTokenDescriptor {
        &ThemeToken::DESCRIPTORS[self.index()]
    }

    /// Whether `key` names a token this version knows about.
    pub fn is_known(key: &str) -> bool {
        ThemeToken::from_name(key).is_some()
    }

    pub fn try_describe(key: &str) -> Option<&'static ThemeTokenDescriptor> {
        ThemeToken::from_name(key).map(ThemeToken::describe)
    }
}
