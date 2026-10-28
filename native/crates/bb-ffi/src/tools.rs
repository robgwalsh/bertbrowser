//! The tool windows' export: one function, a verb and its JSON, the way the `*_do` exports work, so
//! the checksum window, the file compare, disk usage, the change timeline and the folder
//! comparison's banner and sync share one door. The verbs are listed in `bb-app/src/tools.rs`.

use super::*;

/// Runs one tool verb (`checksum.hash`, `filecompare.open`, `diskusage.model`, `changes.open`,
/// `compare.sync`, ...) with its JSON argument (empty for none) and answers JSON. Blocks for as long
/// as the files take: call it from a worker. A verb that starts an operation answers `{op}` and the
/// operation finishes with `BB_EVT_OP_FINISHED`; the vocabulary is `bb-app/src/tools.rs`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tools(
    app: *mut BbApp,
    verb: BbStr,
    args_json: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let verb = unsafe { verb.to_string_lossy() };
        let args = unsafe { args_json.to_string_lossy() };
        let json = app.app.tools(&verb, &args)?;
        *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(&json);
        Ok(())
    })
}
