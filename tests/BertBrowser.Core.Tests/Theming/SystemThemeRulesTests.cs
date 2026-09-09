using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

public class SystemThemeRulesTests
{
    /// <summary>A theme of the user's own, so "the lookup can see it but the catalogue cannot" is
    /// exercised — that is the difference that matters for a theme the user deleted.</summary>
    private static ThemeDefinition UserTheme(string id, bool dark) => new()
    {
        Id = id,
        Name = id,
        BaseThemeId = dark ? ThemeCatalog.DarkPlus.Id : ThemeCatalog.LightPlus.Id,
        IsDark = dark,
    };

    /// <summary>The built-ins plus whatever extra themes a case needs. Anything else is absent, the
    /// way a deleted user theme is.</summary>
    private static Func<string, ThemeDefinition?> Lookup(params ThemeDefinition[] extra) =>
        id => extra.FirstOrDefault(t => string.Equals(t.Id, id, StringComparison.OrdinalIgnoreCase))
              ?? ThemeCatalog.Find(id);

    // ---- who follows, on which launch -------------------------------------------------------

    /// <summary>A brand-new install matches Windows.</summary>
    [Fact]
    public void A_fresh_install_follows()
    {
        Assert.True(SystemThemeRules.ShouldFollow(configured: null, hasSettingsFile: false));
    }

    /// <summary>
    /// And an existing user does not — including the ones who never opened Settings and so have no
    /// theme id either. Keying off the theme id instead would have flipped every one of them on
    /// upgrade, which is the whole reason this takes the file rather than the id.
    /// </summary>
    [Fact]
    public void An_existing_settings_file_does_not_start_following()
    {
        Assert.False(SystemThemeRules.ShouldFollow(configured: null, hasSettingsFile: true));
    }

    [Theory]
    [InlineData(true, true)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    [InlineData(false, false)]
    public void An_explicit_choice_beats_both(bool configured, bool hasFile)
    {
        Assert.Equal(configured, SystemThemeRules.ShouldFollow(configured, hasFile));
    }

    // ---- high contrast wins outright ------------------------------------------------------

    [Fact]
    public void High_contrast_wins_with_no_slots_set()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.HighContrast, null, null, Lookup());

        Assert.Equal(ThemeCatalog.HighContrastDark.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Fact]
    public void High_contrast_wins_over_both_configured_slots()
    {
        var choice = SystemThemeRules.Decide(
            SystemAppearance.HighContrast, "solarized-light", "nord", Lookup());

        Assert.Equal(ThemeCatalog.HighContrastDark.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    /// <summary>The row that catches anyone "simplifying" the early return away: an accessibility
    /// setting must not produce a warning about a slot it never consulted.</summary>
    [Fact]
    public void High_contrast_ignores_a_broken_slot_and_reports_nothing()
    {
        var choice = SystemThemeRules.Decide(
            SystemAppearance.HighContrast, null, "deleted-theme", Lookup());

        Assert.Equal(ThemeCatalog.HighContrastDark.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    // ---- the defaults ---------------------------------------------------------------------

    [Fact]
    public void Dark_with_no_slot_is_dark_plus()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, null, Lookup());

        Assert.Equal(ThemeCatalog.DarkPlus.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Fact]
    public void Light_with_no_slot_is_light_plus()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Light, null, null, Lookup());

        Assert.Equal(ThemeCatalog.LightPlus.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    public void A_blank_slot_is_the_same_as_no_slot(string slot)
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, slot, Lookup());

        Assert.Equal(ThemeCatalog.DarkPlus.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    // ---- the slots ------------------------------------------------------------------------

    [Fact]
    public void Dark_uses_the_dark_slot()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, "nord", Lookup());

        Assert.Equal("nord", choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Fact]
    public void Light_uses_the_light_slot()
    {
        var choice = SystemThemeRules.Decide(
            SystemAppearance.Light, "solarized-light", null, Lookup());

        Assert.Equal("solarized-light", choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Fact]
    public void A_user_theme_the_catalogue_does_not_know_is_still_usable()
    {
        var mine = UserTheme("midnight", dark: true);

        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, "midnight", Lookup(mine));

        Assert.Equal("midnight", choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    [Fact]
    public void Slot_ids_are_matched_case_insensitively()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, "NoRd", Lookup());

        Assert.Equal("nord", choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    /// <summary>The inactive slot is never consulted, so a broken one stays silent until you
    /// switch — which is the only moment it matters.</summary>
    [Fact]
    public void The_inactive_slot_is_not_consulted()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, "no-such-theme", null, Lookup());

        Assert.Equal(ThemeCatalog.DarkPlus.Id, choice.ThemeId);
        Assert.Null(choice.Issue);
    }

    // ---- a slot that has gone missing -------------------------------------------------------

    [Fact]
    public void A_missing_dark_slot_falls_back_and_says_so()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Dark, null, "deleted-theme", Lookup());

        Assert.Equal(ThemeCatalog.DarkPlus.Id, choice.ThemeId);
        Assert.Equal(ThemeIssueSeverity.Error, choice.Issue?.Severity);
        Assert.Contains("deleted-theme", choice.Issue!.Message);
    }

    [Fact]
    public void A_missing_light_slot_falls_back_and_says_so()
    {
        var choice = SystemThemeRules.Decide(
            SystemAppearance.Light, "deleted-theme", null, Lookup());

        Assert.Equal(ThemeCatalog.LightPlus.Id, choice.ThemeId);
        Assert.Equal(ThemeIssueSeverity.Error, choice.Issue?.Severity);
    }

    // ---- a slot holding the wrong kind of theme ---------------------------------------------

    /// <summary>Honoured rather than corrected: IsDark is metadata and the choice is the user's.
    /// Only a hand-edited settings.json reaches here, and repairing it would be the surprise.</summary>
    [Fact]
    public void A_light_theme_in_the_dark_slot_is_honoured_with_a_warning()
    {
        var choice = SystemThemeRules.Decide(
            SystemAppearance.Dark, null, "solarized-light", Lookup());

        Assert.Equal("solarized-light", choice.ThemeId);
        Assert.Equal(ThemeIssueSeverity.Warning, choice.Issue?.Severity);
    }

    [Fact]
    public void A_dark_theme_in_the_light_slot_is_honoured_with_a_warning()
    {
        var choice = SystemThemeRules.Decide(SystemAppearance.Light, "nord", null, Lookup());

        Assert.Equal("nord", choice.ThemeId);
        Assert.Equal(ThemeIssueSeverity.Warning, choice.Issue?.Severity);
    }

    // ---- totality ---------------------------------------------------------------------------

    /// <summary>Like ThemeResolver, this never throws and never produces an unusable answer.</summary>
    [Fact]
    public void Every_combination_yields_a_theme_the_lookup_can_resolve()
    {
        string?[] slots = [null, "", "nord", "solarized-light", "deleted-theme", "midnight"];
        var lookup = Lookup(UserTheme("midnight", dark: true));

        foreach (var appearance in new[]
                 { SystemAppearance.Light, SystemAppearance.Dark, SystemAppearance.HighContrast })
        {
            foreach (var light in slots)
            {
                foreach (var dark in slots)
                {
                    var choice = SystemThemeRules.Decide(appearance, light, dark, lookup);

                    Assert.False(string.IsNullOrWhiteSpace(choice.ThemeId));
                    Assert.NotNull(lookup(choice.ThemeId));
                }
            }
        }
    }

    [Fact]
    public void The_three_answers_it_can_invent_are_real_built_ins()
    {
        Assert.NotNull(ThemeCatalog.Find(SystemThemeRules.DefaultLightThemeId));
        Assert.NotNull(ThemeCatalog.Find(SystemThemeRules.DefaultDarkThemeId));
        Assert.NotNull(ThemeCatalog.Find(SystemThemeRules.HighContrastThemeId));

        Assert.False(ThemeCatalog.Find(SystemThemeRules.DefaultLightThemeId)!.IsDark);
        Assert.True(ThemeCatalog.Find(SystemThemeRules.DefaultDarkThemeId)!.IsDark);
    }

    // ---- what the two pickers offer ----------------------------------------------------------

    [Fact]
    public void The_dark_list_is_every_dark_built_in_except_the_accessibility_one()
    {
        var choices = SystemThemeRules.SlotChoices(ThemeCatalog.BuiltIns, dark: true);

        Assert.All(choices, t => Assert.True(t.IsDark));
        Assert.DoesNotContain(choices, t => t.Id == ThemeCatalog.HighContrastDark.Id);
        Assert.Contains(choices, t => t.Id == ThemeCatalog.DarkPlus.Id);
        Assert.Equal(ThemeCatalog.BuiltIns.Count(t => t.IsDark) - 1, choices.Count);
    }

    [Fact]
    public void The_light_list_is_every_light_built_in()
    {
        var choices = SystemThemeRules.SlotChoices(ThemeCatalog.BuiltIns, dark: false);

        Assert.All(choices, t => Assert.False(t.IsDark));
        Assert.Contains(choices, t => t.Id == ThemeCatalog.LightPlus.Id);
        Assert.Equal(ThemeCatalog.BuiltIns.Count(t => !t.IsDark), choices.Count);
    }

    [Fact]
    public void A_user_theme_appears_in_the_list_for_its_own_side()
    {
        var mine = UserTheme("midnight", dark: true);
        var available = ThemeCatalog.BuiltIns.Append(mine).ToList();

        Assert.Contains(SystemThemeRules.SlotChoices(available, dark: true), t => t.Id == "midnight");
        Assert.DoesNotContain(
            SystemThemeRules.SlotChoices(available, dark: false), t => t.Id == "midnight");
    }

    /// <summary>Or the picker would render empty and the only way to see what was set would be the
    /// settings file.</summary>
    [Fact]
    public void An_assigned_theme_that_fails_the_filter_is_kept_in_the_list_once()
    {
        var choices = SystemThemeRules.SlotChoices(
            ThemeCatalog.BuiltIns, dark: true, assignedId: "solarized-light");

        Assert.Single(choices, t => t.Id == "solarized-light");
        Assert.Equal("solarized-light", choices[0].Id);
    }

    [Fact]
    public void An_assigned_theme_already_in_the_list_is_not_duplicated()
    {
        var choices = SystemThemeRules.SlotChoices(
            ThemeCatalog.BuiltIns, dark: true, assignedId: "nord");

        Assert.Single(choices, t => t.Id == "nord");
    }

    [Fact]
    public void An_assigned_theme_that_does_not_exist_changes_nothing()
    {
        var plain = SystemThemeRules.SlotChoices(ThemeCatalog.BuiltIns, dark: true);
        var withGhost = SystemThemeRules.SlotChoices(
            ThemeCatalog.BuiltIns, dark: true, assignedId: "deleted-theme");

        Assert.Equal(plain.Select(t => t.Id), withGhost.Select(t => t.Id));
    }
}
