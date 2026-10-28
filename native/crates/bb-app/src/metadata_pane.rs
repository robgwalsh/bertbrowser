//! The metadata pane's model: one box per field over whatever is selected, and one undo step per
//! apply. Spec: MetadataViewModel and ShellViewModel.Metadata in the C# app.
//!
//! It reads through the same codecs that write, so what a box shows is exactly what would be
//! written back. A file no codec handles is read and written through Windows' own property handler
//! instead (the host's [`ShellPropertyHost`]) when that handler offers anything, and its row says
//! the result is not checked the way a codec's is.
//!
//! **It never writes on its own.** Apply plans and executes through the queue, which is what puts
//! an edit behind the transfer surface and into the undo history. The file's own modified and
//! created dates are two more boxes like any other; they go through the timestamp executor after
//! the metadata is written, since writing it is itself what moves a modified date.
//!
//! **A field is only written when its text was changed.** Where the items disagree the box starts
//! empty and says so, and leaving it alone leaves every item's own value alone, which is what lets
//! forty songs get one album without losing forty titles. Typing into it and emptying it again is a
//! deliberate clear.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use bb_core::FileTime;
use bb_core::metadata::{
    FileSystemMetadataProbe, MetadataCodecs, MetadataDocument, MetadataEdit, MetadataEditExecutor,
    MetadataEditPlan, MetadataEditPlanner, MetadataError, MetadataFamily, MetadataField,
    MetadataFieldKind, MetadataFields, MetadataRejection, MetadataResult, OfferedByWindows,
    PropertyFallback, PropertyFallbackReading, WindowsPropertyFields, WindowsPropertyValue,
    WritabilityMemory,
};
use bb_core::preview::{PreviewMetadata, ShellPropertyRow};
use bb_core::timestamps::{TimestampChange, TimestampExecutor, TimestampPlanner};
use bb_core::undo::{MetadataEditRecord, TimestampRecord};
use bb_paths::file_name;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult, OpId, TabId};

const MODIFIED_ID: &str = "modified";
const CREATED_ID: &str = "created";

/// Said under every file Windows writes rather than a codec.
pub const VIA_WINDOWS_NOTE: &str = "Written by Windows, not by this app: the fields are checked afterwards, the rest of the file is not.";

// ---- the host's side of the property system ----------------------------------------------------

/// One property's value as the Windows property store reported it.
#[derive(Clone, Debug, Default)]
pub struct HostValue {
    pub canonical: String,
    pub text: Option<String>,
    pub number: Option<f64>,
    pub utc: Option<FileTime>,
}

/// What the host found when asked about a file's writable properties.
#[derive(Clone, Debug, Default)]
pub struct HostReading {
    /// The canonical names the file's handler will write, when the host asked (a store opened for
    /// writing); None when it did not ask or the store would not open.
    pub writable: Option<Vec<String>>,
    pub values: Vec<HostValue>,
}

/// The Windows property system, which only the host can reach. All of these block.
pub trait ShellPropertyHost: Send + Sync {
    /// Every property with a value, display-formatted (`GPS_OPENSLOWITEM`, shared read).
    fn details(&self, path: &str) -> Vec<ShellPropertyRow>;

    /// The values of `canonicals` through an ordinary shared store and, when `ask_writable`, which
    /// of them a store opened for writing says it will take. None when no store would open.
    fn read_fields(
        &self,
        path: &str,
        canonicals: &[&str],
        ask_writable: bool,
    ) -> Option<HostReading>;

    /// Sets the properties and commits, in place. `Err` carries the handler's own words.
    fn write_fields(
        &self,
        path: &str,
        values: &[(String, WindowsPropertyValue)],
    ) -> Result<(), String>;
}

/// [`PropertyFallback`] over the host. Only the plumbing lives here: which property a field is and
/// how a value crosses are `WindowsPropertyFields`, in Core, where they are tested.
pub struct HostFallback {
    host: Arc<dyn ShellPropertyHost>,
    memory: Mutex<WritabilityMemory>,
}

impl HostFallback {
    pub fn new(host: Arc<dyn ShellPropertyHost>) -> HostFallback {
        HostFallback {
            host,
            memory: Mutex::new(WritabilityMemory::new()),
        }
    }
}

impl PropertyFallback for HostFallback {
    fn read(&self, path: &str) -> Option<PropertyFallbackReading> {
        let names: Vec<&str> = WindowsPropertyFields::canonical()
            .iter()
            .map(|(_, n)| *n)
            .collect();
        let recalled = lock(&self.memory)
            .recall(path)
            .map(<[MetadataField]>::to_vec);
        let reading = self.host.read_fields(path, &names, recalled.is_none())?;

        // What a type's handler writes is learnt once per extension: the question costs a
        // write-open, which is exclusive. A store that would not open is not remembered, since
        // that can be this one file being in use.
        let fields: Vec<MetadataField> = match (recalled, reading.writable) {
            (Some(known), _) => known,
            (None, Some(writable)) => {
                let fields: Vec<MetadataField> = writable
                    .iter()
                    .filter_map(|n| WindowsPropertyFields::field_of(n))
                    .filter(|f| WindowsPropertyFields::canonical_of(*f).is_some())
                    .collect();
                lock(&self.memory).remember(path, Some(fields.clone()));
                fields
            }
            (None, None) => return None,
        };
        if fields.is_empty() {
            return None;
        }

        let mut values = BTreeMap::new();
        for value in &reading.values {
            let Some(field) = WindowsPropertyFields::field_of(&value.canonical) else {
                continue;
            };
            if !fields.contains(&field) {
                continue;
            }
            let v = WindowsPropertyFields::from_windows(
                field,
                value.text.as_deref(),
                value.number,
                value.utc,
            );
            if !v.is_empty() {
                values.insert(field, v);
            }
        }
        Some(PropertyFallbackReading {
            document: MetadataDocument::new(MetadataFamily::Document, values),
            fields,
        })
    }

    fn write(&self, path: &str, changes: &BTreeMap<MetadataField, String>) -> MetadataResult<()> {
        let mut values = Vec::new();
        for (field, value) in changes {
            if let Some(name) = WindowsPropertyFields::canonical_of(*field) {
                values.push((
                    name.to_string(),
                    WindowsPropertyFields::to_windows(*field, value)?,
                ));
            }
        }
        self.host.write_fields(path, &values).map_err(|refused| {
            MetadataError::format(format!(
                "Windows could not write to {}: {}.",
                file_name(path),
                refused.trim_end_matches('.')
            ))
        })
    }
}

// ---- the session -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    Ready,
    Skipped,
    Changed,
    Failed,
}

impl FileState {
    fn name(self) -> &'static str {
        match self {
            FileState::Ready => "Ready",
            FileState::Skipped => "Skipped",
            FileState::Changed => "Changed",
            FileState::Failed => "Failed",
        }
    }
}

#[derive(Clone)]
struct MetaFile {
    path: String,
    name: String,
    is_directory: bool,
    state: FileState,
    detail: String,
    document: Option<MetadataDocument>,
    fields: Vec<MetadataField>,
    holds_picture: bool,
    via_windows: bool,
    modified: String,
    created: String,
}

struct FieldRow {
    id: String,
    label: String,
    kind: MetadataFieldKind,
    field: Option<MetadataField>,
    initial: String,
    mixed: bool,
    text: String,
    touched: bool,
}

impl FieldRow {
    fn normalized(&self) -> Option<String> {
        MetadataFields::try_normalize_kind(self.kind, &self.text)
    }

    fn is_dirty(&self) -> bool {
        if self.mixed {
            self.touched
        } else {
            self.normalized().as_deref() != Some(self.initial.as_str())
        }
    }

    /// Not a value of this kind, or an emptied date of the file's own, which can be changed but
    /// never cleared: there is no file without one.
    fn is_invalid(&self) -> bool {
        self.is_dirty()
            && match self.normalized() {
                None => true,
                Some(v) => self.field.is_none() && v.is_empty(),
            }
    }

    /// The canonical value this row would write, when it is one.
    fn value(&self) -> Option<String> {
        if self.is_invalid() {
            None
        } else {
            self.normalized()
        }
    }

    fn shows_mixed_placeholder(&self) -> bool {
        self.mixed && !self.touched && self.text.is_empty()
    }

    fn note(&self) -> &'static str {
        if self.is_invalid() {
            return match self.kind {
                MetadataFieldKind::Number => "Not a number",
                MetadataFieldKind::Date => "Not a date",
                MetadataFieldKind::Rating => "0 to 5",
                _ => "Not allowed",
            };
        }
        if !self.is_dirty() {
            return "";
        }
        if self.value().is_some_and(|v| v.is_empty()) {
            "Will be cleared"
        } else {
            "Changed"
        }
    }

    /// Whether the note earns a line of its own: only where the box cannot say it.
    fn shows_note(&self) -> bool {
        self.is_invalid() || (self.is_dirty() && self.value().is_some_and(|v| v.is_empty()))
    }

    fn hint(&self) -> Option<&'static str> {
        match self.kind {
            MetadataFieldKind::Date => Some("A date and time, as 2024-03-14 09:26:53"),
            MetadataFieldKind::List => Some("Separate each one with a semicolon"),
            MetadataFieldKind::Rating => Some("Stars, 0 to 5"),
            _ => None,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            MetadataFieldKind::Text => "text",
            MetadataFieldKind::Number => "number",
            MetadataFieldKind::Date => "date",
            MetadataFieldKind::List => "list",
            MetadataFieldKind::Rating => "rating",
            MetadataFieldKind::Decimal => "decimal",
            MetadataFieldKind::Exposure => "exposure",
        }
    }
}

#[derive(Default)]
pub(crate) struct MetadataSession {
    paths: Vec<String>,
    signature: String,
    files: Vec<MetaFile>,
    fields: Vec<FieldRow>,
    details: Vec<(String, String)>,
    /// What the last apply did to each item, kept while the same items stay selected: the list
    /// reloads itself after every write, and a failure's reason must outlive that.
    results: HashMap<String, (FileState, String)>,
    outcome_summary: Option<String>,
    busy: bool,
    loaded: bool,
    picture: Option<Vec<u8>>,
    picture_note: String,
}

fn upper(s: &str) -> String {
    s.to_uppercase()
}

fn count(items: usize) -> String {
    if items == 1 {
        "1 item".into()
    } else {
        format!("{items} items")
    }
}

fn local_text(time: std::time::SystemTime) -> String {
    MetadataFields::format_date(FileTime::from_system_time(time).to_local())
}

impl MetadataSession {
    fn editable(&self) -> impl Iterator<Item = &MetaFile> {
        self.files.iter().filter(|f| f.document.is_some())
    }

    /// What a removal reaches: only what a codec writes. Taking something out is a promise about
    /// the whole file, and Windows' handlers are not read behind.
    fn removable(&self) -> impl Iterator<Item = &MetaFile> {
        self.editable().filter(|f| !f.via_windows)
    }

    fn located(&self) -> usize {
        self.editable()
            .filter(|f| f.document.as_ref().is_some_and(|d| d.has_location))
            .count()
    }

    fn can_hold_picture(&self) -> bool {
        self.editable().any(|f| f.holds_picture)
    }

    fn can_apply(&self) -> bool {
        !self.busy
            && self.fields.iter().any(FieldRow::is_dirty)
            && !self.fields.iter().any(FieldRow::is_invalid)
    }

    fn can_remove_location(&self) -> bool {
        !self.busy && self.located() > 0
    }

    fn can_remove_all(&self) -> bool {
        !self.busy && self.removable().next().is_some()
    }

    fn can_remove_picture(&self) -> bool {
        !self.busy
            && self.editable().any(|f| {
                f.holds_picture && f.document.as_ref().is_some_and(|d| d.picture.is_some())
            })
    }

    fn build_fields(&mut self) {
        self.fields.clear();
        let editable: Vec<&MetaFile> = self.editable().collect();
        let mut rows = Vec::new();
        for spec in MetadataFields::all() {
            let held: Vec<String> = editable
                .iter()
                .filter(|f| f.fields.contains(&spec.field))
                .map(|f| {
                    f.document
                        .as_ref()
                        .map(|d| d.get(spec.field).to_string())
                        .unwrap_or_default()
                })
                .collect();
            if held.is_empty() {
                continue;
            }
            rows.push(row(spec.id, spec.label, spec.kind, Some(spec.field), held));
        }
        // Every item has these, whatever it is: which is why they come last. On a text file or a
        // folder they are the whole pane.
        let dated: Vec<&MetaFile> = self
            .files
            .iter()
            .filter(|f| !f.modified.is_empty())
            .collect();
        if !dated.is_empty() {
            rows.push(row(
                MODIFIED_ID,
                "Date modified",
                MetadataFieldKind::Date,
                None,
                dated.iter().map(|f| f.modified.clone()).collect(),
            ));
            rows.push(row(
                CREATED_ID,
                "Date created",
                MetadataFieldKind::Date,
                None,
                dated.iter().map(|f| f.created.clone()).collect(),
            ));
        }
        self.fields = rows;
        self.build_picture();
    }

    fn build_picture(&mut self) {
        let holders: Vec<&MetaFile> = self.editable().filter(|f| f.holds_picture).collect();
        let covers: Vec<Option<&Vec<u8>>> = holders
            .iter()
            .map(|f| f.document.as_ref().and_then(|d| d.picture.as_ref()))
            .collect();
        let shared = match covers.first() {
            Some(Some(first)) if covers.iter().all(|c| c.is_some_and(|c| c == *first)) => {
                Some((*first).clone())
            }
            _ => None,
        };
        self.picture_note = if holders.is_empty() || shared.is_some() {
            String::new()
        } else if covers.iter().all(Option::is_none) {
            "No cover picture.".into()
        } else {
            "(multiple pictures)".into()
        };
        self.picture = shared;
    }

    fn dirty_count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_dirty()).count()
    }

    fn summary(&self) -> String {
        if let Some(outcome) = &self.outcome_summary {
            return outcome.clone();
        }
        if self.fields.iter().any(FieldRow::is_invalid) {
            return "Fix the highlighted field before applying.".into();
        }
        match self.dirty_count() {
            0 => String::new(),
            1 => "1 field will be written.".into(),
            n => format!("{n} fields will be written."),
        }
    }

    fn title(&self) -> String {
        match self.paths.len() {
            0 => "Metadata".into(),
            1 => {
                let name = file_name(&self.paths[0]);
                if name.is_empty() {
                    self.paths[0].clone()
                } else {
                    name.to_string()
                }
            }
            n => format!("{n} items"),
        }
    }

    /// The metadata edit the boxes describe: every changed field, and nothing else.
    fn current_edit(&self) -> MetadataEdit {
        let changes: BTreeMap<MetadataField, String> = self
            .fields
            .iter()
            .filter_map(|f| match (f.field, f.is_dirty(), f.value()) {
                (Some(field), true, Some(v)) => Some((field, v)),
                _ => None,
            })
            .collect();
        MetadataEdit::none().with_changes(changes)
    }

    fn dirty_date(&self, id: &str) -> Option<FileTime> {
        let row = self.fields.iter().find(|f| f.id == id)?;
        if !row.is_dirty() {
            return None;
        }
        let value = row.value()?;
        if value.is_empty() {
            return None;
        }
        MetadataFields::try_parse_date(&value)
    }
}

fn row(
    id: &str,
    label: &str,
    kind: MetadataFieldKind,
    field: Option<MetadataField>,
    held: Vec<String>,
) -> FieldRow {
    let mut distinct: Vec<String> = Vec::new();
    for v in held {
        if !distinct.contains(&v) {
            distinct.push(v);
        }
    }
    let mixed = distinct.len() > 1;
    let initial = if mixed {
        String::new()
    } else {
        distinct.first().cloned().unwrap_or_default()
    };
    FieldRow {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        field,
        text: initial.clone(),
        initial,
        mixed,
        touched: false,
    }
}

// ---- reading one item --------------------------------------------------------------------------

fn attributes_of(path: &str) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| m.file_attributes())
}

fn read_one(path: &str, fallback: Option<&dyn PropertyFallback>) -> MetaFile {
    use bb_core::attributes::DIRECTORY;
    let attributes = attributes_of(path);
    let is_directory = attributes.is_some_and(|a| a & DIRECTORY != 0);
    let mut file = MetaFile {
        path: path.to_string(),
        name: file_name(path).to_string(),
        is_directory,
        state: FileState::Ready,
        detail: String::new(),
        document: None,
        fields: Vec::new(),
        holds_picture: false,
        via_windows: false,
        modified: String::new(),
        created: String::new(),
    };
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(m) = meta.modified() {
            file.modified = local_text(m);
        }
        if let Ok(c) = meta.created() {
            file.created = local_text(c);
        }
    }

    if is_directory {
        file.state = FileState::Skipped;
        file.detail = "A folder holds no metadata; only its dates can be changed.".into();
        return file;
    }

    if let Some(refusal) = MetadataEditPlanner::refusal(path, attributes, false) {
        // No codec is not the end of it: Windows may have a handler that writes this type. Asked
        // only once every other refusal has passed, since asking opens the file.
        if refusal.reason == MetadataRejection::Unsupported
            && MetadataEditPlanner::refusal(path, attributes, true).is_none()
            && let Some(fb) = fallback
            && let Some(offered) = fb.read(path)
        {
            file.document = Some(offered.document);
            file.fields = offered.fields;
            file.via_windows = true;
            file.detail = VIA_WINDOWS_NOTE.into();
            return file;
        }
        file.state = FileState::Skipped;
        file.detail = if refusal.reason == MetadataRejection::Unsupported {
            format!(
                "{}; only its dates can be changed.",
                refusal.message.trim_end_matches('.')
            )
        } else {
            refusal.message
        };
        return file;
    }

    let Some(codec) = MetadataCodecs::for_path(path) else {
        file.state = FileState::Skipped;
        return file;
    };
    let Some(mut stream) = bb_core::fs::ReadOnlyFile::try_open(path, false) else {
        file.state = FileState::Skipped;
        file.detail = format!("{} could not be opened.", file.name);
        return file;
    };
    match codec.read(&mut stream) {
        Ok(document) => {
            file.document = Some(document);
            file.fields = codec.fields().to_vec();
            file.holds_picture = codec.holds_picture();
        }
        Err(MetadataError::Format(message)) => {
            file.state = FileState::Skipped;
            file.detail = message;
        }
        Err(e) => {
            file.state = FileState::Skipped;
            file.detail = format!("{} could not be read: {e}", file.name);
        }
    }
    file
}

/// The tags a rename template may name, read from the file's own metadata. Empty for a folder, a
/// file in an archive, or one no codec reads: the template then refuses that item by name.
pub(crate) fn tags_of(path: &str, is_directory: bool) -> HashMap<MetadataField, String> {
    if is_directory || MetadataEditPlanner::is_inside_archive(path) {
        return HashMap::new();
    }
    let Some(codec) = MetadataCodecs::for_path(path) else {
        return HashMap::new();
    };
    let Some(mut stream) = bb_core::fs::ReadOnlyFile::try_open(path, false) else {
        return HashMap::new();
    };
    codec
        .read(&mut stream)
        .map(|d| d.values.into_iter().collect())
        .unwrap_or_default()
}

// ---- the app's side ----------------------------------------------------------------------------

/// What a press of one of the pane's buttons asks for.
pub enum MetadataAction {
    Apply,
    RemoveLocation,
    RemoveAll,
    RemovePicture,
    SetPicture(Vec<u8>),
}

type SessionCell = Arc<Mutex<MetadataSession>>;

#[derive(Default)]
pub(crate) struct MetadataState {
    sessions: Mutex<HashMap<TabId, SessionCell>>,
    pub host: Mutex<Option<Arc<dyn ShellPropertyHost>>>,
    fallback: Mutex<Option<Arc<HostFallback>>>,
}

impl MetadataState {
    fn session(&self, tab: TabId) -> SessionCell {
        lock(&self.sessions).entry(tab).or_default().clone()
    }

    fn fallback(&self) -> Option<Arc<HostFallback>> {
        let host = lock(&self.host).clone()?;
        let mut slot = lock(&self.fallback);
        Some(
            slot.get_or_insert_with(|| Arc::new(HostFallback::new(host)))
                .clone(),
        )
    }

    pub fn set_host(&self, host: Option<Arc<dyn ShellPropertyHost>>) {
        *lock(&self.host) = host;
        *lock(&self.fallback) = None;
    }
}

impl Inner {
    /// The selection's paths and a signature of them with their modified times, so a refresh that
    /// changes neither does not wipe what is half typed.
    fn metadata_selection(&self, tab: TabId) -> AppResult<(Vec<String>, String)> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        let mut paths = Vec::new();
        let mut signature = String::new();
        for r in t
            .listing
            .rows()
            .iter()
            .filter(|r| t.selection.contains(&r.id))
        {
            paths.push(r.full_path.clone());
            signature.push_str(&format!("{}|{}\n", upper(&r.full_path), r.modified.0));
        }
        Ok((paths, signature))
    }

    /// Points the pane at the tab's selection and reads it (blocking). Does nothing when neither
    /// the paths nor their modified times moved, unless `force`.
    pub(crate) fn metadata_load(&self, tab: TabId, force: bool) -> AppResult<()> {
        let (paths, signature) = self.metadata_selection(tab)?;
        self.metadata_load_paths(tab, paths, signature, force);
        Ok(())
    }

    fn metadata_load_paths(&self, tab: TabId, paths: Vec<String>, signature: String, force: bool) {
        let cell = self.extra.metadata.session(tab);
        {
            let mut s = lock(&cell);
            if !force && s.loaded && s.signature == signature {
                return;
            }
            let same_items = paths.len() == s.paths.len()
                && paths
                    .iter()
                    .zip(&s.paths)
                    .all(|(a, b)| a.eq_ignore_ascii_case(b));
            if !same_items {
                s.results.clear();
                s.outcome_summary = None;
            }
            s.paths = paths.clone();
            s.signature = signature;
        }

        let fallback = self.extra.metadata.fallback();
        let mut files: Vec<MetaFile> = paths
            .iter()
            .map(|p| read_one(p, fallback.as_deref().map(|f| f as &dyn PropertyFallback)))
            .collect();

        let host = lock(&self.extra.metadata.host).clone();
        let details = match (&host, files.as_slice()) {
            (Some(host), [only])
                if !only.is_directory
                    && attributes_of(&only.path)
                        .is_some_and(bb_core::columns::MetadataReadRules::may_read)
                    && !MetadataEditPlanner::is_inside_archive(&only.path) =>
            {
                let rows: Vec<ShellPropertyRow> = host
                    .details(&only.path)
                    .into_iter()
                    // What has a box below is not said twice: this list is for what nobody can type.
                    .filter(|p| {
                        WindowsPropertyFields::field_of(&p.canonical)
                            .is_none_or(|f| !only.fields.contains(&f))
                    })
                    .collect();
                PreviewMetadata::details(&rows)
                    .into_iter()
                    .map(|r| (r.label, r.value))
                    .collect()
            }
            _ => Vec::new(),
        };

        let mut s = lock(&cell);
        for file in &mut files {
            if let Some((state, detail)) = s.results.get(&file.path).cloned() {
                file.state = state;
                // A file Windows writes keeps saying so, whatever the last apply did to it.
                if !detail.is_empty() || !file.via_windows {
                    file.detail = detail;
                }
            }
        }
        s.files = files;
        s.details = details;
        s.build_fields();
        s.loaded = true;
    }

    pub(crate) fn metadata_model(&self, tab: TabId) -> AppResult<Value> {
        let cell = self.extra.metadata.session(tab);
        let s = lock(&cell);
        let files: Vec<Value> = s
            .files
            .iter()
            .map(|f| {
                json!({
                    "name": f.name, "path": f.path, "isDirectory": f.is_directory,
                    "state": f.state.name(), "detail": f.detail, "viaWindows": f.via_windows,
                })
            })
            .collect();
        let fields: Vec<Value> = s
            .fields
            .iter()
            .map(|f| {
                let picked = if f.kind == MetadataFieldKind::Date {
                    MetadataFields::try_parse_date(f.text.trim()).map(|d| {
                        let c = d.civil();
                        format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
                    })
                } else {
                    None
                };
                json!({
                    "id": f.id, "label": f.label, "kind": f.kind_name(), "isMixed": f.mixed,
                    "text": f.text, "mixedPlaceholder": f.shows_mixed_placeholder(),
                    "isDirty": f.is_dirty(), "isInvalid": f.is_invalid(),
                    "note": f.note(), "showsNote": f.shows_note(), "hint": f.hint(),
                    "isDate": f.kind == MetadataFieldKind::Date, "pickedDate": picked,
                })
            })
            .collect();
        let located = s.located();
        let editable = s.editable().count();
        let location_note = match located {
            0 => "Nothing here says where it was made.".to_string(),
            1 if editable == 1 => "This picture says where it was taken.".to_string(),
            1 => "1 of these says where it was taken.".to_string(),
            n => format!("{n} of these say where they were taken."),
        };
        Ok(json!({
            "title": s.title(), "summary": s.summary(), "isBusy": s.busy,
            "isEmpty": s.files.is_empty(), "files": files, "fields": fields,
            "details": s.details.iter().map(|(l, v)| json!({"label": l, "value": v})).collect::<Vec<_>>(),
            "canHoldPicture": s.can_hold_picture(), "hasPicture": s.picture.is_some(),
            "pictureNote": s.picture_note,
            "located": located, "locationNote": location_note,
            "canApply": s.can_apply(), "canRemove": s.removable().next().is_some(),
            "canRemoveLocation": s.can_remove_location(), "canRemoveAll": s.can_remove_all(),
            "canRemovePicture": s.can_remove_picture(),
        }))
    }

    /// The cover the selection shares, as the image file it is.
    pub(crate) fn metadata_picture(&self, tab: TabId) -> Option<Vec<u8>> {
        lock(&self.extra.metadata.session(tab)).picture.clone()
    }

    pub(crate) fn metadata_set_text(&self, tab: TabId, id: &str, text: &str) -> AppResult<()> {
        let cell = self.extra.metadata.session(tab);
        let mut s = lock(&cell);
        let row = s
            .fields
            .iter_mut()
            .find(|f| f.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| {
                AppError::Invalid(format!("the pane has no '{id}' box for this selection"))
            })?;
        if row.text != text {
            row.text = text.to_string();
            row.touched = true;
            s.outcome_summary = None;
        }
        Ok(())
    }

    pub(crate) fn metadata_revert(&self, tab: TabId, id: &str) -> AppResult<()> {
        let cell = self.extra.metadata.session(tab);
        let mut s = lock(&cell);
        let row = s
            .fields
            .iter_mut()
            .find(|f| f.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| {
                AppError::Invalid(format!("the pane has no '{id}' box for this selection"))
            })?;
        row.text = row.initial.clone();
        row.touched = false;
        s.outcome_summary = None;
        Ok(())
    }

    /// Picking a day in the calendar keeps the time already in the box: a calendar has no clock,
    /// and choosing a day must not move a photograph to midnight.
    pub(crate) fn metadata_pick_date(
        &self,
        tab: TabId,
        id: &str,
        year: i32,
        month: u32,
        day: u32,
    ) -> AppResult<()> {
        let cell = self.extra.metadata.session(tab);
        let text = {
            let s = lock(&cell);
            let row = s
                .fields
                .iter()
                .find(|f| f.id.eq_ignore_ascii_case(id))
                .ok_or_else(|| AppError::Invalid(format!("the pane has no '{id}' box")))?;
            let time_of = |text: &str| {
                MetadataFields::try_parse_date(text.trim()).map(|d| {
                    let c = d.civil();
                    (c.hour, c.minute, c.second)
                })
            };
            let (h, m, sec) = time_of(&row.text)
                .or_else(|| time_of(&row.initial))
                .unwrap_or((0, 0, 0));
            MetadataFields::format_date(FileTime::from_ymd_hms(
                i64::from(year),
                month,
                day,
                h,
                m,
                sec,
            ))
        };
        self.metadata_set_text(tab, id, &text)
    }

    /// Runs one of the pane's buttons through the queue. The files' fate lands in the session when
    /// the operation finishes, and `Model::Metadata` says so.
    pub(crate) fn metadata_act(
        self: &Arc<Self>,
        tab: TabId,
        action: MetadataAction,
    ) -> AppResult<OpId> {
        let cell = self.extra.metadata.session(tab);
        let (edit, modified, created, targets, all_paths, skipped, via) = {
            let mut s = lock(&cell);
            let refuse = |what: &str, s: &MetadataSession| {
                AppError::Invalid(format!(
                    "the metadata pane would not {what}: {}",
                    s.summary()
                ))
            };
            let (edit, modified, created) = match &action {
                MetadataAction::Apply => {
                    if !s.can_apply() {
                        return Err(refuse("apply", &s));
                    }
                    (
                        s.current_edit(),
                        s.dirty_date(MODIFIED_ID),
                        s.dirty_date(CREATED_ID),
                    )
                }
                MetadataAction::RemoveLocation => {
                    if !s.can_remove_location() {
                        return Err(refuse("remove the location", &s));
                    }
                    (
                        MetadataEdit {
                            remove_location: true,
                            ..MetadataEdit::none()
                        },
                        None,
                        None,
                    )
                }
                MetadataAction::RemoveAll => {
                    if !s.can_remove_all() {
                        return Err(refuse("remove everything", &s));
                    }
                    (
                        MetadataEdit {
                            remove_all: true,
                            ..MetadataEdit::none()
                        },
                        None,
                        None,
                    )
                }
                MetadataAction::RemovePicture => {
                    if !s.can_remove_picture() {
                        return Err(refuse("remove the picture", &s));
                    }
                    (
                        MetadataEdit {
                            remove_picture: true,
                            ..MetadataEdit::none()
                        },
                        None,
                        None,
                    )
                }
                MetadataAction::SetPicture(bytes) => {
                    if s.busy || !s.can_hold_picture() {
                        return Err(refuse("set a picture", &s));
                    }
                    (
                        MetadataEdit {
                            set_picture: Some(bytes.clone()),
                            ..MetadataEdit::none()
                        },
                        None,
                        None,
                    )
                }
            };
            if edit.is_empty() && modified.is_none() && created.is_none() {
                return Err(AppError::Invalid("nothing to apply".into()));
            }
            let targets: Vec<String> = if edit.changes.is_empty() {
                s.removable().map(|f| f.path.clone()).collect()
            } else {
                s.editable().map(|f| f.path.clone()).collect()
            };
            let via: OfferedByWindows = s
                .files
                .iter()
                .filter(|f| {
                    f.via_windows && targets.iter().any(|t| t.eq_ignore_ascii_case(&f.path))
                })
                .map(|f| (f.path.clone(), f.fields.clone()))
                .collect();
            let skipped: Vec<(String, String)> = s
                .files
                .iter()
                .filter(|f| f.state == FileState::Skipped)
                .map(|f| (f.path.clone(), f.detail.clone()))
                .collect();
            let all_paths: Vec<String> = s.files.iter().map(|f| f.path.clone()).collect();
            s.busy = true;
            (edit, modified, created, targets, all_paths, skipped, via)
        };

        let plan = if edit.is_empty() {
            MetadataEditPlan::empty()
        } else {
            MetadataEditPlanner::local().plan(&targets, &edit, Some(&via))
        };

        let submitted = self.submit(
            "Metadata edit",
            false,
            Box::new({
                let cell = cell.clone();
                move |inner, _op, cancel| {
                    let result = inner.run_metadata(
                        tab, &cell, &plan, &all_paths, skipped, modified, created, cancel,
                    );
                    lock(&cell).busy = false;
                    inner.hub.model(Model::Metadata);
                    result
                }
            }),
        );
        if submitted.is_err() {
            lock(&cell).busy = false;
        }
        submitted
    }

    #[allow(clippy::too_many_arguments)]
    fn run_metadata(
        self: &Arc<Self>,
        tab: TabId,
        cell: &SessionCell,
        plan: &MetadataEditPlan,
        all_paths: &[String],
        skipped: Vec<(String, String)>,
        modified: Option<FileTime>,
        created: Option<FileTime>,
        cancel: &bb_core::cancel::CancelToken,
    ) -> (bool, String) {
        let mut results: HashMap<String, (FileState, String)> = HashMap::new();
        for (path, detail) in skipped {
            results.insert(path, (FileState::Skipped, detail));
        }
        for rejected in &plan.rejected {
            results.insert(
                rejected.path.clone(),
                (FileState::Ready, rejected.message.clone()),
            );
        }

        let mut failed = 0usize;
        let mut status = String::new();
        let mut folders: Vec<String> = Vec::new();

        if plan.has_work() {
            let description = if plan.files.len() == 1 {
                file_name(&plan.files[0].path).to_string()
            } else {
                format!("{} files", plan.files.len())
            };
            self.set_active_status(format!("Writing metadata to {description}\u{2026}"));
            folders.extend(
                plan.files
                    .iter()
                    .filter_map(|f| bb_core::rename::paths::directory_name(&f.path)),
            );

            let fallback = self.extra.metadata.fallback();
            let executor = MetadataEditExecutor::new(
                &FileSystemMetadataProbe,
                fallback.as_deref().map(|f| f as &dyn PropertyFallback),
            );
            let outcome = executor.execute(plan, cancel, None, Some(&self.gate));
            self.record(MetadataEditRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));
            for edited in &outcome.edited {
                results.insert(edited.path.clone(), (FileState::Changed, String::new()));
            }
            for failure in &outcome.failed {
                results.insert(
                    failure.path.clone(),
                    (FileState::Failed, failure.message.clone()),
                );
            }
            failed += outcome.failed.len();
            status = {
                let changed = if outcome.edited.len() == 1 {
                    format!("{} updated", file_name(&outcome.edited[0].path))
                } else {
                    format!("{} files updated", outcome.edited.len())
                };
                if outcome.failed.len() == 1 && outcome.edited.is_empty() {
                    outcome.failed[0].message.clone()
                } else if !outcome.failed.is_empty() {
                    format!("{changed}; {} could not be changed.", outcome.failed.len())
                } else if outcome.cancelled {
                    if outcome.edited.is_empty() {
                        "Nothing was changed.".into()
                    } else {
                        format!("{changed}, then stopped.")
                    }
                } else {
                    format!("{changed}.")
                }
            };
        }

        // After the metadata, never before: rewriting a file is what moves its modified date, so a
        // date set first would be overwritten by the very edit it was applied with.
        for (value, is_modified) in [(modified, true), (created, false)] {
            let Some(date) = value else { continue };
            let plan = TimestampPlanner::local().plan(
                all_paths,
                &TimestampChange::value(is_modified, !is_modified, date),
            );
            if !plan.has_work() {
                continue;
            }
            let outcome = TimestampExecutor::local().execute(&plan);
            self.record(TimestampRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));
            folders.extend(
                plan.items
                    .iter()
                    .filter_map(|i| bb_core::rename::paths::directory_name(&i.path)),
            );
            for stamped in &outcome.stamped {
                let already_failed = results
                    .get(&stamped.path)
                    .is_some_and(|r| r.0 == FileState::Failed);
                if !already_failed {
                    results.insert(stamped.path.clone(), (FileState::Changed, String::new()));
                }
            }
            for failure in &outcome.failed {
                results.insert(
                    failure.path.clone(),
                    (FileState::Failed, failure.message.clone()),
                );
            }
            failed += outcome.failed.len();
            status = if outcome.stamped.len() == 1 {
                format!(
                    "The date of {} was changed.",
                    file_name(&outcome.stamped[0].path)
                )
            } else {
                format!("The dates of {} items were changed.", outcome.stamped.len())
            };
        }

        let touched = results
            .values()
            .filter(|r| r.0 == FileState::Changed)
            .count();
        let summary = if touched == 0 && failed == 0 {
            "Nothing needed changing.".to_string()
        } else if failed > 0 {
            format!(
                "{} changed; {failed} could not be. Undo takes it back.",
                count(touched)
            )
        } else {
            format!("{} changed. Undo takes it back.", count(touched))
        };
        if !status.is_empty() {
            self.set_active_status(status);
        }

        let paths = {
            let mut s = lock(cell);
            s.results = results;
            s.outcome_summary = Some(summary.clone());
            s.paths.clone()
        };

        // The listing reloads behind this, and the pane answers its own edit by selecting what it
        // edited again, or it would go blank.
        if let Ok(tab_cell) = self.tab(tab) {
            lock(&tab_cell).pending_selection = paths.clone();
        }
        folders.sort_by_key(|d| upper(d));
        folders.dedup_by_key(|d| upper(d));
        self.refresh_tabs_showing(&folders);

        // Read again rather than patched in place: the boxes then show what the items hold now,
        // which for a failed one is what it held before.
        let (_, signature) = self.metadata_selection(tab).unwrap_or_default();
        let signature = if signature.is_empty() {
            paths.iter().map(|p| format!("{}|-\n", upper(p))).collect()
        } else {
            signature
        };
        self.metadata_load_paths(tab, paths, signature, true);
        (failed == 0, summary)
    }
}

impl Inner {
    /// What a file holds now, read from disk through the codec that writes it (or, for a type no codec
    /// handles, the Windows handler): one field by id, or `location` / `picture` as "yes" or "". For
    /// the harness and tests, which check a write by reading it back a second way.
    pub(crate) fn metadata_peek(&self, path: &str, what: &str) -> Result<String, String> {
        let document = if let Some(codec) = MetadataCodecs::for_path(path) {
            let mut stream = bb_core::fs::ReadOnlyFile::try_open(path, false).ok_or_else(|| {
                format!(
                    "'{}' could not be opened to read its tags.",
                    file_name(path)
                )
            })?;
            codec.read(&mut stream).map_err(|e| e.to_string())?
        } else {
            let fallback = self.extra.metadata.fallback();
            fallback
                .as_deref()
                .and_then(|f| f.read(path))
                .map(|r| r.document)
                .ok_or_else(|| format!("nothing reads the metadata of '{}'.", file_name(path)))?
        };
        match what.to_ascii_lowercase().as_str() {
            "location" => Ok(if document.has_location { "yes" } else { "" }.to_string()),
            "picture" => Ok(if document.picture.is_some() {
                "yes"
            } else {
                ""
            }
            .to_string()),
            id => {
                let spec = MetadataFields::find(id)
                    .ok_or_else(|| format!("'{id}' is not a metadata field."))?;
                Ok(document.get(spec.field).to_string())
            }
        }
    }
}
