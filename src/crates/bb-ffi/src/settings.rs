//! Settings and theme.

use super::*;

// ---- settings and theme ----

/// One setting as JSON text (`null` for an unset optional). Keys are the settings file's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_get(
    app: *mut BbApp,
    key: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_json = out(out_json, "out_json")?;
        *out_json = BbOwnedStr::from_str(&app.app.settings_get(&key.to_string_lossy())?);
        Ok(())
    })
}

/// Sets one setting from JSON text, validating it against the model. It takes effect at once; a
/// setting the Settings page owns is written on a 400 ms debounce (`bb_settings_flush` writes it
/// now), one a service owns (keymap, bars, theme) is written at once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_set(
    app: *mut BbApp,
    key: BbStr,
    json_value: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(app_of(app)?
            .app
            .settings_set(&key.to_string_lossy(), &json_value.to_string_lossy())?)
    })
}

/// How many colour tokens a palette holds.
#[unsafe(no_mangle)]
pub extern "C" fn bb_theme_token_count() -> u32 {
    bb_theming::ThemeToken::COUNT as u32
}

/// The current theme's colours as 0xAARRGGBB, one per token in token order. Writes up to `cap` and
/// reports how many exist in `out_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_palette(
    app: *mut BbApp,
    out_argb: *mut u32,
    cap: u32,
    out_count: *mut u32,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let palette = app.app.theme_palette();
        copy_to(&palette[..palette.len().min(cap as usize)], out_argb);
        set_opt(out_count, palette.len() as u32);
        Ok(())
    })
}
