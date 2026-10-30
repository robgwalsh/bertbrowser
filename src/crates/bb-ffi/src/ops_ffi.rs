//! The exports of the operations UI: the transfer queue and its window, a clash put to the user,
//! the offer of administrator rights, the undo history, drag and drop, the right-drag menu, rename
//! preview, extract / compress / shortcut and the composed right-click menu.
//!
//! Two doors, as everywhere else in this ABI: `bb_ops_query` reads a model as JSON and
//! `bb_ops_do` runs a verb. The vocabulary is documented on `App::ops_query` / `App::ops_do`;
//! results are freed with `bb_str_free`.

use super::*;

/// A model of the operations UI as JSON text: `queue`, `history`, `conflicts`, `elevation`,
/// `right-drop-menu`, `drop-allowed`, `drop-decide`, `dragout-decide`, `rename-preview`,
/// `context-menu`, `menu-arrange`, `menu-tidy`, `compress-defaults`, `archive-of`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_ops_query(
    app: *mut BbApp,
    what: BbStr,
    arg: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let text = app
            .app
            .ops_query(&unsafe { what.to_string_lossy() }, &unsafe {
                arg.to_string_lossy()
            })?;
        *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(&text);
        Ok(())
    })
}

/// A verb of the operations UI. `out_json` (when non-NULL) receives `{"op": N}` for a verb that
/// started work, `{}` otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_ops_do(
    app: *mut BbApp,
    verb: BbStr,
    arg: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let text = app
            .app
            .ops_do(&unsafe { verb.to_string_lossy() }, &unsafe {
                arg.to_string_lossy()
            })?;
        if let Some(o) = unsafe { out_json.as_mut() } {
            *o = BbOwnedStr::from_str(&text);
        }
        Ok(())
    })
}
