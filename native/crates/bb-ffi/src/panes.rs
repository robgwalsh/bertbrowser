//! The exports for the side panes: the preview's model, the animated GIF it plays, the metadata
//! pane of a tab, the host's window onto the Windows property system, rename by rule, and the
//! fixtures the harness writes.
//!
//! **Two exceptions to "every call is made from the UI thread and none blocks".** The preview
//! reads a file's front and a GIF waits for its first frame, so `bb_preview_build*` and
//! `bb_gif_open` are meant for a worker thread of the host's; both are safe from any thread. The
//! property host's callbacks run on whichever Rust thread needed the answer (the UI thread loading
//! the pane, the transfer thread writing), so the host initialises COM for the thread it is called
//! on. Strings a callback returns are views into storage the host keeps until its next call on that
//! thread; Rust copies them at once.

use super::*;

use bb_app::{
    GifOpenError, HostReading, HostValue, MODE_AUTO, MODE_GUESS, MODE_HEX, MODE_RAW,
    MetadataAction, ShellPropertyHost,
};
use bb_core::FileTime;
use bb_core::metadata::WindowsPropertyValue;
use bb_core::preview::ShellPropertyRow;
use serde_json::{Value, json};

fn text_of(s: BbStr) -> String {
    unsafe { s.to_string_lossy() }
}

unsafe fn put(out_json: *mut BbOwnedStr, text: &str) -> Result<(), Fail> {
    *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(text);
    Ok(())
}

// ---- the preview ----

/// What the preview pane shows for the tab's selection, as JSON `{state, title, subtitle, message,
/// canChooseMode, path, lines, spans, footer, entries, ...}`; see `bb-app/src/preview.rs`. `mode` is
/// 0 auto, 1 raw text, 2 hex, 3 the document fallback (the shell declined; read the bytes as text).
/// Blocking: call it from a worker thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_preview_build(
    app: *mut BbApp,
    tab: u64,
    mode: u32,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let mode = match mode {
            1 => MODE_RAW,
            2 => MODE_HEX,
            3 => MODE_GUESS,
            _ => MODE_AUTO,
        };
        let json = app.app.preview_build(TabId(tab), mode)?;
        unsafe { put(out_json, &json) }
    })
}

/// As `bb_preview_build` for one path that need not be selected anywhere.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_preview_build_path(
    app: *mut BbApp,
    path: BbStr,
    mode: u32,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let mode = match mode {
            1 => MODE_RAW,
            2 => MODE_HEX,
            3 => MODE_GUESS,
            _ => MODE_AUTO,
        };
        let json = app.app.preview_build_path(&text_of(path), mode);
        unsafe { put(out_json, &json) }
    })
}

// ---- animated GIFs ----

/// Starts playing the GIF at `path` and returns once its first frame is ready; the rest decode on
/// a thread of their own. `out_json` is `{animated, handle, width, height, frames, durationMs}`, or
/// `{animated: false, reason: "still"|"tooLarge"|"unreadable", frames}` with `handle` 0 when it
/// should be shown as a picture. Blocking: call it from a worker thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_open(
    app: *mut BbApp,
    path: BbStr,
    out_handle: *mut u64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let (handle, json) = match app.app.gif_open(&text_of(path)) {
            Ok((handle, player)) => (
                handle,
                json!({
                    "animated": true, "handle": handle, "width": player.width,
                    "height": player.height, "frames": player.frame_count(),
                    "durationMs": player.duration_ms(),
                }),
            ),
            Err(reason) => (
                0,
                match reason {
                    GifOpenError::Still => {
                        json!({"animated": false, "reason": "still", "frames": 1})
                    }
                    GifOpenError::TooLarge { frames } => {
                        json!({"animated": false, "reason": "tooLarge", "frames": frames})
                    }
                    GifOpenError::Unreadable => {
                        json!({"animated": false, "reason": "unreadable", "frames": 0})
                    }
                },
            ),
        };
        if let Some(out_handle) = unsafe { out_handle.as_mut() } {
            *out_handle = handle;
        }
        unsafe { put(out_json, &json.to_string()) }
    })
}

/// How many frames have been decoded, and `flags`: bit 0 all of them, bit 1 decoding ended early
/// (nothing more is coming).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_ready(
    app: *mut BbApp,
    handle: u64,
    out_ready: *mut u32,
    out_flags: *mut u32,
) -> BbStatus {
    guard(|| {
        let player = unsafe { app_of(app)? }.app.gif(handle)?;
        let ready = player.ready();
        let flags = u32::from(player.is_complete()) | (u32::from(player.is_stopped()) << 1);
        if let Some(r) = unsafe { out_ready.as_mut() } {
            *r = ready as u32;
        }
        if let Some(f) = unsafe { out_flags.as_mut() } {
            *f = flags;
        }
        Ok(())
    })
}

/// Copies frame `index` (BGRA, `width * height * 4` bytes) into `dst`, or the last one decoded if it
/// has not arrived yet; `out_index` says which it really was.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_frame(
    app: *mut BbApp,
    handle: u64,
    index: u32,
    dst: *mut u8,
    capacity: usize,
    out_index: *mut u32,
) -> BbStatus {
    guard(|| {
        let player = unsafe { app_of(app)? }.app.gif(handle)?;
        let (at, pixels) = player.frame_or_earlier(index as usize);
        if dst.is_null() || capacity < pixels.len() {
            return Err(Fail::Err(format!("a frame needs {} bytes", pixels.len())));
        }
        unsafe { ptr::copy_nonoverlapping(pixels.as_ptr(), dst, pixels.len()) };
        if let Some(i) = unsafe { out_index.as_mut() } {
            *i = at as u32;
        }
        Ok(())
    })
}

/// The loop arithmetic: the frame on screen at `position_ms` (any position; it wraps), how long
/// until the frame changes (at least 1), and where `frame` starts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_position(
    app: *mut BbApp,
    handle: u64,
    position_ms: i64,
    out_frame: *mut u32,
    out_until_next: *mut i64,
) -> BbStatus {
    guard(|| {
        let player = unsafe { app_of(app)? }.app.gif(handle)?;
        if let Some(f) = unsafe { out_frame.as_mut() } {
            *f = player.frame_at(position_ms) as u32;
        }
        if let Some(n) = unsafe { out_until_next.as_mut() } {
            *n = player.until_next_frame(position_ms);
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_start_of(
    app: *mut BbApp,
    handle: u64,
    frame: u32,
    out_ms: *mut i64,
) -> BbStatus {
    guard(|| {
        let player = unsafe { app_of(app)? }.app.gif(handle)?;
        if let Some(m) = unsafe { out_ms.as_mut() } {
            *m = player.start_of(frame as usize);
        }
        Ok(())
    })
}

/// Stops the decoding and lets the frames go.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_gif_close(app: *mut BbApp, handle: u64) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.gif_close(handle);
        Ok(())
    })
}

// ---- the Windows property system, which only the host can reach ----

/// The host's side of the property system. Every callback returns `BB_OK` with `*out` set to a JSON
/// view that stays valid until the host's next call on that thread, or a failure status.
///
/// - `details(user, path, out)`: `[{canonical, name, value}]` for every property with a value.
/// - `read_fields(user, path, canonicals_json, ask_writable, out)`: `null` when no store would open,
///   else `{writable: [canonical]|null, values: [{canonical, text?, number?, utc?}]}`; `utc` is a
///   FILETIME as a 64-bit integer. `writable` is only filled when `ask_writable` (it costs an
///   exclusive write-open).
/// - `write_fields(user, path, values_json, out_error)`: sets `[{canonical, text?|number?|utc?}]`
///   and commits in place; a failure puts the handler's words in `*out_error`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbPropertyHost {
    pub user: *mut c_void,
    pub details:
        Option<unsafe extern "C" fn(user: *mut c_void, path: BbStr, out: *mut BbStr) -> BbStatus>,
    pub read_fields: Option<
        unsafe extern "C" fn(
            user: *mut c_void,
            path: BbStr,
            canonicals_json: BbStr,
            ask_writable: bool,
            out: *mut BbStr,
        ) -> BbStatus,
    >,
    pub write_fields: Option<
        unsafe extern "C" fn(
            user: *mut c_void,
            path: BbStr,
            values_json: BbStr,
            out_error: *mut BbStr,
        ) -> BbStatus,
    >,
}

unsafe impl Send for BbPropertyHost {}
unsafe impl Sync for BbPropertyHost {}

struct CHost(BbPropertyHost);

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn parse_text(view: BbStr) -> Option<Value> {
    serde_json::from_str(&text_of(view)).ok()
}

impl ShellPropertyHost for CHost {
    fn details(&self, path: &str) -> Vec<ShellPropertyRow> {
        let Some(call) = self.0.details else {
            return Vec::new();
        };
        let path = units(path);
        let mut view = BbStr::EMPTY;
        if unsafe { call(self.0.user, BbStr::of(&path), &mut view) } != BbStatus::BB_OK {
            return Vec::new();
        }
        let Some(Value::Array(rows)) = parse_text(view) else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|r| {
                Some(ShellPropertyRow {
                    canonical: r["canonical"].as_str()?.to_string(),
                    name: r["name"].as_str()?.to_string(),
                    value: r["value"].as_str()?.to_string(),
                })
            })
            .collect()
    }

    fn read_fields(
        &self,
        path: &str,
        canonicals: &[&str],
        ask_writable: bool,
    ) -> Option<HostReading> {
        let call = self.0.read_fields?;
        let path = units(path);
        let names = units(&json!(canonicals).to_string());
        let mut view = BbStr::EMPTY;
        let status = unsafe {
            call(
                self.0.user,
                BbStr::of(&path),
                BbStr::of(&names),
                ask_writable,
                &mut view,
            )
        };
        if status != BbStatus::BB_OK {
            return None;
        }
        let v = parse_text(view)?;
        if v.is_null() {
            return None;
        }
        let writable = v["writable"].as_array().map(|a| {
            a.iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        });
        let values = v["values"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(HostValue {
                            canonical: x["canonical"].as_str()?.to_string(),
                            text: x["text"].as_str().map(str::to_string),
                            number: x["number"].as_f64(),
                            utc: x["utc"].as_i64().map(FileTime),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(HostReading { writable, values })
    }

    fn write_fields(
        &self,
        path: &str,
        values: &[(String, WindowsPropertyValue)],
    ) -> Result<(), String> {
        let Some(call) = self.0.write_fields else {
            return Err("Windows cannot be reached".into());
        };
        let rows: Vec<Value> = values
            .iter()
            .map(|(name, v)| {
                let mut row = json!({"canonical": name});
                if let Some(t) = &v.text {
                    row["text"] = json!(t);
                } else if let Some(n) = v.number {
                    row["number"] = json!(n);
                } else if let Some(u) = v.utc {
                    row["utc"] = json!(u.0);
                }
                row
            })
            .collect();
        let path = units(path);
        let body = units(&Value::Array(rows).to_string());
        let mut error = BbStr::EMPTY;
        if unsafe { call(self.0.user, BbStr::of(&path), BbStr::of(&body), &mut error) }
            == BbStatus::BB_OK
        {
            Ok(())
        } else {
            let words = text_of(error);
            Err(if words.is_empty() {
                "Windows refused".into()
            } else {
                words
            })
        }
    }
}

/// Installs the host's window onto the Windows property system (all callbacks null removes it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_set_property_host(app: *mut BbApp, host: BbPropertyHost) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let installed =
            host.details.is_some() || host.read_fields.is_some() || host.write_fields.is_some();
        app.app.set_shell_property_host(
            installed.then(|| Arc::new(CHost(host)) as Arc<dyn ShellPropertyHost>),
        );
        Ok(())
    })
}

// ---- the metadata pane ----

/// Points the tab's metadata pane at its selection and reads it (blocking, but only as long as the
/// files take). Nothing happens when neither the selected paths nor their modified times moved,
/// unless `force`: a refresh that changes neither must not wipe what is half typed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_load(app: *mut BbApp, tab: u64, force: bool) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .metadata_load(TabId(tab), force)?)
    })
}

/// The pane as JSON: `{title, summary, isBusy, isEmpty, files, fields, details, canHoldPicture,
/// hasPicture, pictureNote, located, locationNote, canApply, canRemove, canRemoveLocation,
/// canRemoveAll, canRemovePicture}`; see `bb-app/src/metadata_pane.rs`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_model(
    app: *mut BbApp,
    tab: u64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let model = unsafe { app_of(app)? }.app.metadata_model(TabId(tab))?;
        unsafe { put(out_json, &model.to_string()) }
    })
}

/// Types into a box. Only a change of the text counts as touching it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_set_text(
    app: *mut BbApp,
    tab: u64,
    id: BbStr,
    text: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }.app.metadata_set_text(
            TabId(tab),
            &text_of(id),
            &text_of(text),
        )?)
    })
}

/// Puts a box back to what the files hold.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_revert(app: *mut BbApp, tab: u64, id: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .metadata_revert(TabId(tab), &text_of(id))?)
    })
}

/// A day chosen in a date box's calendar; the time already in the box is kept.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_pick_date(
    app: *mut BbApp,
    tab: u64,
    id: BbStr,
    year: i32,
    month: u32,
    day: u32,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }.app.metadata_pick_date(
            TabId(tab),
            &text_of(id),
            year,
            month,
            day,
        )?)
    })
}

/// Presses one of the pane's buttons: 0 Apply, 1 remove the location, 2 remove everything, 3 remove
/// the cover picture, 4 set the cover picture to the `len` bytes at `data`. Queued like any write
/// and recorded in the undo history; `BB_ERR` says why the button would not have worked. The pane
/// updates itself when the operation finishes and `BB_EVT_MODEL_CHANGED` 13 arrives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_act(
    app: *mut BbApp,
    tab: u64,
    action: u32,
    data: *const u8,
    len: usize,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let action = match action {
            0 => MetadataAction::Apply,
            1 => MetadataAction::RemoveLocation,
            2 => MetadataAction::RemoveAll,
            3 => MetadataAction::RemovePicture,
            4 if !data.is_null() => MetadataAction::SetPicture(
                unsafe { std::slice::from_raw_parts(data, len) }.to_vec(),
            ),
            _ => return Err(Fail::Err("unknown metadata action".into())),
        };
        let op = app.app.metadata_act(TabId(tab), action)?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// The cover the selection shares, as the image file it is. Call with `dst` null to learn the
/// length; `*out_len` is 0 when there is none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_picture(
    app: *mut BbApp,
    tab: u64,
    dst: *mut u8,
    capacity: usize,
    out_len: *mut usize,
) -> BbStatus {
    guard(|| {
        let picture = unsafe { app_of(app)? }
            .app
            .metadata_picture(TabId(tab))
            .unwrap_or_default();
        if let Some(l) = unsafe { out_len.as_mut() } {
            *l = picture.len();
        }
        if !dst.is_null() && capacity >= picture.len() {
            unsafe { ptr::copy_nonoverlapping(picture.as_ptr(), dst, picture.len()) };
        }
        Ok(())
    })
}

// ---- comparing two panes ----

/// Reads both sides of each path and settles its verdict to "same" where the bytes are identical;
/// nothing else moves. `out_identical` is how many were. Blocking: call it from a worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_compare_settle(
    app: *mut BbApp,
    paths: *const BbStr,
    n: u32,
    out_identical: *mut u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let identical = app.app.compare_settle(&unsafe { strings(paths, n) })?;
        if let Some(o) = unsafe { out_identical.as_mut() } {
            *o = identical as u32;
        }
        Ok(())
    })
}

// ---- rename by rule, and the history ----

/// Renames by the expanded panel's rule, given as the settings file spells it:
/// `{template, find, replace, useRegex, matchCase, scope, case, counterStart, counterStep}` (a
/// missing key keeps its default). A template that names a tag has each file's metadata read first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_rename_rule(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    rule_json: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let rule: bb_core::rename::RenameRule = serde_json::from_str(&text_of(rule_json))
            .map_err(|e| Fail::Err(format!("not a rename rule: {e}")))?;
        let op = app.app.op_rename_rule(unsafe { strings(src, n) }, rule)?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Commits what the undo history is holding and empties it (Settings, History, Clear).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_undo_clear(app: *mut BbApp) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.release_undo_history();
        Ok(())
    })
}

// ---- fixtures the harness writes ----

/// Writes a set of small real files into `dir` (created if need be): `metadata` (a camera picture,
/// a bare picture, two songs), `media` (the same tone as FLAC, WAV, M4A and WMA, and a two-track
/// MP4), `formats` (PNG, WebP, TIFF, a Word document, an EPUB, AIFF and Opus), `windows` (an `.asf`
/// and a GIF no codec writes). None of them is written by the code under test: Windows' own
/// encoders, or bytes laid out by hand from the specifications.
#[cfg(feature = "fixtures")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_fixture_write(kind: BbStr, dir: BbStr) -> BbStatus {
    use bb_test_support::{metadata_fixtures as m, metadata_fixtures_data as d};
    guard(|| {
        let dir = std::path::PathBuf::from(text_of(dir));
        std::fs::create_dir_all(&dir).map_err(|e| Fail::Err(e.to_string()))?;
        let put = |name: &str, bytes: Vec<u8>| -> Result<(), Fail> {
            std::fs::write(dir.join(name), bytes).map_err(|e| Fail::Err(e.to_string()))
        };
        match text_of(kind).as_str() {
            "metadata" => {
                put("camera.jpg", m::jpeg(d::CAMERA_JPEG))?;
                put("plain.jpg", m::jpeg(d::PLAIN_JPEG))?;
                put(
                    "first.mp3",
                    m::mp3(
                        &[
                            ("TIT2", "First song"),
                            ("TPE1", "The Band"),
                            ("TALB", "Album One"),
                            ("TRCK", "1"),
                        ],
                        false,
                    ),
                )?;
                put(
                    "second.mp3",
                    m::mp3(
                        &[
                            ("TIT2", "Second song"),
                            ("TPE1", "The Band"),
                            ("TALB", "Album One"),
                            ("TRCK", "2"),
                        ],
                        true,
                    ),
                )?;
            }
            "media" => {
                put("tone.flac", m::bytes(d::FLAC))?;
                put("tone.m4a", m::bytes(d::M4A))?;
                put("tone.wma", m::bytes(d::WMA))?;
                put("tone.wav", m::wav())?;
                put("clip.mp4", m::bytes(d::MP4_VIDEO))?;
            }
            "formats" => {
                put("pixel.png", m::bytes(d::PNG))?;
                put("pixel.webp", m::bytes(d::WEBP))?;
                put("report.docx", m::docx())?;
                put("book.epub", m::epub())?;
                put("scan.tif", m::bytes(d::TIFF))?;
                put("tone.aiff", m::aiff())?;
                put("tone.opus", m::opus())?;
            }
            "windows" => {
                put("tone.asf", m::bytes(d::WMA))?;
                put(
                    "dot.gif",
                    m::bytes("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="),
                )?;
            }
            other => return Err(Fail::Err(format!("'{other}' is not a fixture set"))),
        }
        Ok(())
    })
}

// ---- image zoom and the media playlist ----

/// The scale after `notches` wheel notches, relative to "fit" (1). Pure arithmetic: the view owns
/// the transform, the core owns the rules.
#[unsafe(no_mangle)]
pub extern "C" fn bb_zoom_step(scale: f64, notches: f64, min_scale: f64) -> f64 {
    bb_core::preview::ImageZoom::step(scale, notches, min_scale)
}

#[unsafe(no_mangle)]
pub extern "C" fn bb_zoom_clamp(scale: f64, min_scale: f64) -> f64 {
    bb_core::preview::ImageZoom::clamp(scale, min_scale)
}

/// The offset that keeps the image point under the cursor where it is when the scale changes.
#[unsafe(no_mangle)]
pub extern "C" fn bb_zoom_offset(offset: f64, anchor: f64, old_scale: f64, new_scale: f64) -> f64 {
    bb_core::preview::ImageZoom::zoom_offset(offset, anchor, old_scale, new_scale)
}

/// Keeps the picture from being dragged out of view (centred where it is smaller than the pane).
#[unsafe(no_mangle)]
pub extern "C" fn bb_zoom_clamp_offset(
    offset: f64,
    origin: f64,
    length: f64,
    scale: f64,
    viewport: f64,
) -> f64 {
    bb_core::preview::ImageZoom::clamp_offset(offset, origin, length, scale, viewport)
}

/// The row to select to go on to the next video after the selected one (`-1`: there is none).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_preview_next_media(
    app: *mut BbApp,
    tab: u64,
    out_row: *mut i64,
) -> BbStatus {
    guard(|| {
        let next = unsafe { app_of(app)? }.app.preview_next_media(TabId(tab))?;
        if let Some(o) = unsafe { out_row.as_mut() } {
            *o = next.map_or(-1, i64::from);
        }
        Ok(())
    })
}

/// What the file holds now, read from disk through the codec that writes it: one field by id, or
/// `location` / `picture` as "yes" or an empty string. `BB_ERR` says why it could not be read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_metadata_peek(
    app: *mut BbApp,
    path: BbStr,
    what: BbStr,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let value = app
            .app
            .metadata_peek(&text_of(path), &text_of(what))
            .map_err(Fail::Err)?;
        unsafe { put(out_text, &value) }
    })
}
