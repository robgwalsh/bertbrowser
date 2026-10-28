//! How each record crosses the pipe: one compact JSON document per line, written and read by the
//! same code at both ends so they cannot disagree about a field. Spec: the `ElevationJson` options
//! and `Write`/`Read` helpers of Core/Services/Elevation/ElevationHost.cs and ElevationClient.cs.
//!
//! The domain types carry no serialization of their own, so each has a mirror here with the C#
//! field names. Compact and on one line: the framing depends on the payload carrying no raw
//! newline, and indenting would put one in.

use serde::{Deserialize, Serialize};

use super::models::{
    ElevationHeader, ElevationNewItemResult, ElevationOperation, ElevationProgressReport,
    ElevationSummary, ElevationTransferItem, ElevationTransferResult,
};
use crate::FileTime;
use crate::delete::{DeleteDisposition, DeleteMode, DeletedItem, FailedDelete, PlannedDelete};
use crate::fs::EntryStamp;
use crate::newitem::{FailedNewItem, NewItemKind, NewItemPlan, NewItemRejection, RejectedNewItem};
use crate::rename::{CompletedRename, FailedRename, PlannedRename};
use crate::transfer::{
    ClashSide, CompletedTransfer, ConflictResolution, FailedTransfer, PlannedTransfer,
    TransferClash,
};

/// A record that can be put on one line of the pipe and read back.
pub trait WireRecord: Sized {
    fn to_json(&self) -> String;

    /// None for anything that does not parse: a malformed line from the helper must not lose the
    /// ones around it, and a malformed request must be refused rather than carried out in part.
    fn from_json(text: &str) -> Option<Self>;
}

/// Implements `WireRecord` for a domain type through a serde mirror with `From` both ways.
macro_rules! wire {
    ($domain:ty => $mirror:ty) => {
        impl WireRecord for $domain {
            fn to_json(&self) -> String {
                serde_json::to_string(&<$mirror>::from(self)).unwrap_or_default()
            }

            fn from_json(text: &str) -> Option<Self> {
                serde_json::from_str::<$mirror>(text)
                    .ok()
                    .map(<$domain>::from)
            }
        }
    };
}

// --- the elevation models themselves ---

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HeaderWire {
    operation: ElevationOperation,
    destination_directory: String,
    delete_mode: DeleteMode,
    permanent: bool,
}

impl Serialize for ElevationOperation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            ElevationOperation::TransferMove => "TransferMove",
            ElevationOperation::TransferCopy => "TransferCopy",
            ElevationOperation::Delete => "Delete",
            ElevationOperation::Rename => "Rename",
            ElevationOperation::NewItem => "NewItem",
            ElevationOperation::TransferUndo => "TransferUndo",
            ElevationOperation::DeleteUndo => "DeleteUndo",
        })
    }
}

impl<'de> Deserialize<'de> for ElevationOperation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(match text.as_str() {
            "TransferMove" => ElevationOperation::TransferMove,
            "TransferCopy" => ElevationOperation::TransferCopy,
            "Delete" => ElevationOperation::Delete,
            "Rename" => ElevationOperation::Rename,
            "NewItem" => ElevationOperation::NewItem,
            "TransferUndo" => ElevationOperation::TransferUndo,
            "DeleteUndo" => ElevationOperation::DeleteUndo,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "{other} is not an elevation operation"
                )));
            }
        })
    }
}

impl Serialize for DeleteMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            DeleteMode::Recycle => "Recycle",
            DeleteMode::Staged => "Staged",
            DeleteMode::Permanent => "Permanent",
        })
    }
}

impl<'de> Deserialize<'de> for DeleteMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(match text.as_str() {
            "Recycle" => DeleteMode::Recycle,
            "Staged" => DeleteMode::Staged,
            "Permanent" => DeleteMode::Permanent,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "{other} is not a delete mode"
                )));
            }
        })
    }
}

impl From<&ElevationHeader> for HeaderWire {
    fn from(h: &ElevationHeader) -> Self {
        HeaderWire {
            operation: h.operation,
            destination_directory: h.destination_directory.clone(),
            delete_mode: h.delete_mode,
            permanent: h.permanent,
        }
    }
}

impl From<HeaderWire> for ElevationHeader {
    fn from(w: HeaderWire) -> Self {
        ElevationHeader {
            operation: w.operation,
            destination_directory: w.destination_directory,
            delete_mode: w.delete_mode,
            permanent: w.permanent,
        }
    }
}

wire!(ElevationHeader => HeaderWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TransferItemWire {
    item: PlannedTransferWire,
    resolution: ConflictResolutionWire,
}

impl From<&ElevationTransferItem> for TransferItemWire {
    fn from(i: &ElevationTransferItem) -> Self {
        TransferItemWire {
            item: PlannedTransferWire::from(&i.item),
            resolution: ConflictResolutionWire::from(i.resolution),
        }
    }
}

impl From<TransferItemWire> for ElevationTransferItem {
    fn from(w: TransferItemWire) -> Self {
        ElevationTransferItem {
            item: PlannedTransfer::from(w.item),
            resolution: ConflictResolution::from(w.resolution),
        }
    }
}

wire!(ElevationTransferItem => TransferItemWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TransferResultWire {
    completed: Option<CompletedTransferWire>,
    skipped: Option<String>,
}

impl From<&ElevationTransferResult> for TransferResultWire {
    fn from(r: &ElevationTransferResult) -> Self {
        TransferResultWire {
            completed: r.completed.as_ref().map(CompletedTransferWire::from),
            skipped: r.skipped.clone(),
        }
    }
}

impl From<TransferResultWire> for ElevationTransferResult {
    fn from(w: TransferResultWire) -> Self {
        ElevationTransferResult {
            completed: w.completed.map(CompletedTransfer::from),
            skipped: w.skipped,
        }
    }
}

wire!(ElevationTransferResult => TransferResultWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NewItemResultWire {
    created_path: String,
}

impl From<&ElevationNewItemResult> for NewItemResultWire {
    fn from(r: &ElevationNewItemResult) -> Self {
        NewItemResultWire {
            created_path: r.created_path.clone(),
        }
    }
}

impl From<NewItemResultWire> for ElevationNewItemResult {
    fn from(w: NewItemResultWire) -> Self {
        ElevationNewItemResult {
            created_path: w.created_path,
        }
    }
}

wire!(ElevationNewItemResult => NewItemResultWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProgressWire {
    done: usize,
    total: usize,
    current_name: String,
    bytes_done: i64,
    current_item_bytes: i64,
    current_item_total: i64,
}

impl From<&ElevationProgressReport> for ProgressWire {
    fn from(p: &ElevationProgressReport) -> Self {
        ProgressWire {
            done: p.done,
            total: p.total,
            current_name: p.current_name.clone(),
            bytes_done: p.bytes_done,
            current_item_bytes: p.current_item_bytes,
            current_item_total: p.current_item_total,
        }
    }
}

impl From<ProgressWire> for ElevationProgressReport {
    fn from(w: ProgressWire) -> Self {
        ElevationProgressReport {
            done: w.done,
            total: w.total,
            current_name: w.current_name,
            bytes_done: w.bytes_done,
            current_item_bytes: w.current_item_bytes,
            current_item_total: w.current_item_total,
        }
    }
}

wire!(ElevationProgressReport => ProgressWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SummaryWire {
    cancelled: bool,
    staging_directories: Vec<String>,
    restored: usize,
}

impl From<&ElevationSummary> for SummaryWire {
    fn from(s: &ElevationSummary) -> Self {
        SummaryWire {
            cancelled: s.cancelled,
            staging_directories: s.staging_directories.clone(),
            restored: s.restored,
        }
    }
}

impl From<SummaryWire> for ElevationSummary {
    fn from(w: SummaryWire) -> Self {
        ElevationSummary {
            cancelled: w.cancelled,
            staging_directories: w.staging_directories,
            restored: w.restored,
        }
    }
}

wire!(ElevationSummary => SummaryWire);

// --- transfer ---

#[derive(Serialize, Deserialize, Clone, Copy)]
enum ConflictResolutionWire {
    Skip,
    KeepBoth,
    Replace,
    Overwrite,
}

impl From<ConflictResolution> for ConflictResolutionWire {
    fn from(r: ConflictResolution) -> Self {
        match r {
            ConflictResolution::Skip => ConflictResolutionWire::Skip,
            ConflictResolution::KeepBoth => ConflictResolutionWire::KeepBoth,
            ConflictResolution::Replace => ConflictResolutionWire::Replace,
            ConflictResolution::Overwrite => ConflictResolutionWire::Overwrite,
        }
    }
}

impl From<ConflictResolutionWire> for ConflictResolution {
    fn from(w: ConflictResolutionWire) -> Self {
        match w {
            ConflictResolutionWire::Skip => ConflictResolution::Skip,
            ConflictResolutionWire::KeepBoth => ConflictResolution::KeepBoth,
            ConflictResolutionWire::Replace => ConflictResolution::Replace,
            ConflictResolutionWire::Overwrite => ConflictResolution::Overwrite,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ClashSideWire {
    is_directory: bool,
    bytes: i64,
    modified_utc: i64,
}

impl From<&ClashSide> for ClashSideWire {
    fn from(s: &ClashSide) -> Self {
        ClashSideWire {
            is_directory: s.is_directory,
            bytes: s.bytes,
            modified_utc: s.modified_utc.0,
        }
    }
}

impl From<ClashSideWire> for ClashSide {
    fn from(w: ClashSideWire) -> Self {
        ClashSide {
            is_directory: w.is_directory,
            bytes: w.bytes,
            modified_utc: FileTime(w.modified_utc),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ClashWire {
    incoming: ClashSideWire,
    existing: Option<ClashSideWire>,
    identical: bool,
    incoming_is_newer: bool,
}

impl From<&TransferClash> for ClashWire {
    fn from(c: &TransferClash) -> Self {
        ClashWire {
            incoming: ClashSideWire::from(&c.incoming),
            existing: c.existing.as_ref().map(ClashSideWire::from),
            identical: c.identical,
            incoming_is_newer: c.incoming_is_newer,
        }
    }
}

impl From<ClashWire> for TransferClash {
    fn from(w: ClashWire) -> Self {
        TransferClash {
            incoming: ClashSide::from(w.incoming),
            existing: w.existing.map(ClashSide::from),
            identical: w.identical,
            incoming_is_newer: w.incoming_is_newer,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlannedTransferWire {
    source_path: String,
    is_directory: bool,
    destination_path: String,
    conflicts: bool,
    known_bytes: Option<i64>,
    clash: Option<ClashWire>,
    merge_declined: bool,
}

impl From<&PlannedTransfer> for PlannedTransferWire {
    fn from(t: &PlannedTransfer) -> Self {
        PlannedTransferWire {
            source_path: t.source_path.clone(),
            is_directory: t.is_directory,
            destination_path: t.destination_path.clone(),
            conflicts: t.conflicts,
            known_bytes: t.known_bytes,
            clash: t.clash.as_ref().map(ClashWire::from),
            merge_declined: t.merge_declined,
        }
    }
}

impl From<PlannedTransferWire> for PlannedTransfer {
    fn from(w: PlannedTransferWire) -> Self {
        PlannedTransfer {
            source_path: w.source_path,
            is_directory: w.is_directory,
            destination_path: w.destination_path,
            conflicts: w.conflicts,
            known_bytes: w.known_bytes,
            clash: w.clash.map(TransferClash::from),
            merge_declined: w.merge_declined,
        }
    }
}

wire!(PlannedTransfer => PlannedTransferWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StampWire {
    length: i64,
    write_utc: i64,
    created_utc: i64,
}

impl From<&EntryStamp> for StampWire {
    fn from(s: &EntryStamp) -> Self {
        StampWire {
            length: s.length,
            write_utc: s.write_utc.0,
            created_utc: s.created_utc.0,
        }
    }
}

impl From<StampWire> for EntryStamp {
    fn from(w: StampWire) -> Self {
        EntryStamp {
            length: w.length,
            write_utc: FileTime(w.write_utc),
            created_utc: FileTime(w.created_utc),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CompletedTransferWire {
    source_path: String,
    final_path: String,
    is_directory: bool,
    displaced_stage_path: Option<String>,
    written: Option<StampWire>,
}

impl From<&CompletedTransfer> for CompletedTransferWire {
    fn from(c: &CompletedTransfer) -> Self {
        CompletedTransferWire {
            source_path: c.source_path.clone(),
            final_path: c.final_path.clone(),
            is_directory: c.is_directory,
            displaced_stage_path: c.displaced_stage_path.clone(),
            written: c.written.as_ref().map(StampWire::from),
        }
    }
}

impl From<CompletedTransferWire> for CompletedTransfer {
    fn from(w: CompletedTransferWire) -> Self {
        CompletedTransfer {
            source_path: w.source_path,
            final_path: w.final_path,
            is_directory: w.is_directory,
            displaced_stage_path: w.displaced_stage_path,
            written: w.written.map(EntryStamp::from),
        }
    }
}

wire!(CompletedTransfer => CompletedTransferWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FailedTransferWire {
    source_path: String,
    message: String,
    access_denied: bool,
}

impl From<&FailedTransfer> for FailedTransferWire {
    fn from(f: &FailedTransfer) -> Self {
        FailedTransferWire {
            source_path: f.source_path.clone(),
            message: f.message.clone(),
            access_denied: f.access_denied,
        }
    }
}

impl From<FailedTransferWire> for FailedTransfer {
    fn from(w: FailedTransferWire) -> Self {
        FailedTransfer {
            source_path: w.source_path,
            message: w.message,
            access_denied: w.access_denied,
        }
    }
}

wire!(FailedTransfer => FailedTransferWire);

// --- delete ---

#[derive(Serialize, Deserialize, Clone, Copy)]
enum DispositionWire {
    Recycle,
    Stage,
    Erase,
}

impl From<DeleteDisposition> for DispositionWire {
    fn from(d: DeleteDisposition) -> Self {
        match d {
            DeleteDisposition::Recycle => DispositionWire::Recycle,
            DeleteDisposition::Stage => DispositionWire::Stage,
            DeleteDisposition::Erase => DispositionWire::Erase,
        }
    }
}

impl From<DispositionWire> for DeleteDisposition {
    fn from(w: DispositionWire) -> Self {
        match w {
            DispositionWire::Recycle => DeleteDisposition::Recycle,
            DispositionWire::Stage => DeleteDisposition::Stage,
            DispositionWire::Erase => DeleteDisposition::Erase,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlannedDeleteWire {
    source_path: String,
    is_directory: bool,
    disposition: DispositionWire,
}

impl From<&PlannedDelete> for PlannedDeleteWire {
    fn from(d: &PlannedDelete) -> Self {
        PlannedDeleteWire {
            source_path: d.source_path.clone(),
            is_directory: d.is_directory,
            disposition: DispositionWire::from(d.disposition),
        }
    }
}

impl From<PlannedDeleteWire> for PlannedDelete {
    fn from(w: PlannedDeleteWire) -> Self {
        PlannedDelete::new(&w.source_path, w.is_directory)
            .with_disposition(DeleteDisposition::from(w.disposition))
    }
}

wire!(PlannedDelete => PlannedDeleteWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeletedItemWire {
    source_path: String,
    is_directory: bool,
    staged_path: Option<String>,
    recycled_path: Option<String>,
}

impl From<&DeletedItem> for DeletedItemWire {
    fn from(d: &DeletedItem) -> Self {
        DeletedItemWire {
            source_path: d.source_path.clone(),
            is_directory: d.is_directory,
            staged_path: d.staged_path.clone(),
            recycled_path: d.recycled_path.clone(),
        }
    }
}

impl From<DeletedItemWire> for DeletedItem {
    fn from(w: DeletedItemWire) -> Self {
        DeletedItem {
            source_path: w.source_path,
            is_directory: w.is_directory,
            staged_path: w.staged_path,
            recycled_path: w.recycled_path,
        }
    }
}

wire!(DeletedItem => DeletedItemWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FailedDeleteWire {
    source_path: String,
    message: String,
    access_denied: bool,
}

impl From<&FailedDelete> for FailedDeleteWire {
    fn from(f: &FailedDelete) -> Self {
        FailedDeleteWire {
            source_path: f.source_path.clone(),
            message: f.message.clone(),
            access_denied: f.access_denied,
        }
    }
}

impl From<FailedDeleteWire> for FailedDelete {
    fn from(w: FailedDeleteWire) -> Self {
        FailedDelete {
            source_path: w.source_path,
            message: w.message,
            access_denied: w.access_denied,
        }
    }
}

wire!(FailedDelete => FailedDeleteWire);

// --- rename ---

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlannedRenameWire {
    source_path: String,
    target_path: String,
    is_directory: bool,
}

impl From<&PlannedRename> for PlannedRenameWire {
    fn from(r: &PlannedRename) -> Self {
        PlannedRenameWire {
            source_path: r.source_path.clone(),
            target_path: r.target_path.clone(),
            is_directory: r.is_directory,
        }
    }
}

impl From<PlannedRenameWire> for PlannedRename {
    fn from(w: PlannedRenameWire) -> Self {
        PlannedRename::new(&w.source_path, &w.target_path, w.is_directory)
    }
}

wire!(PlannedRename => PlannedRenameWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CompletedRenameWire {
    source_path: String,
    final_path: String,
    is_directory: bool,
}

impl From<&CompletedRename> for CompletedRenameWire {
    fn from(r: &CompletedRename) -> Self {
        CompletedRenameWire {
            source_path: r.source_path.clone(),
            final_path: r.final_path.clone(),
            is_directory: r.is_directory,
        }
    }
}

impl From<CompletedRenameWire> for CompletedRename {
    fn from(w: CompletedRenameWire) -> Self {
        CompletedRename {
            source_path: w.source_path,
            final_path: w.final_path,
            is_directory: w.is_directory,
        }
    }
}

wire!(CompletedRename => CompletedRenameWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FailedRenameWire {
    source_path: String,
    message: String,
    access_denied: bool,
    stranded_path: Option<String>,
}

impl From<&FailedRename> for FailedRenameWire {
    fn from(f: &FailedRename) -> Self {
        FailedRenameWire {
            source_path: f.source_path.clone(),
            message: f.message.clone(),
            access_denied: f.access_denied,
            stranded_path: f.stranded_path.clone(),
        }
    }
}

impl From<FailedRenameWire> for FailedRename {
    fn from(w: FailedRenameWire) -> Self {
        FailedRename {
            source_path: w.source_path,
            message: w.message,
            access_denied: w.access_denied,
            stranded_path: w.stranded_path,
        }
    }
}

wire!(FailedRename => FailedRenameWire);

// --- new item ---

#[derive(Serialize, Deserialize, Clone, Copy)]
enum NewItemKindWire {
    Folder,
    File,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
enum NewItemRejectionWire {
    InvalidName,
    NameTaken,
    ParentMissing,
    TemplateMissing,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RejectedNewItemWire {
    reason: NewItemRejectionWire,
    message: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NewItemPlanWire {
    directory: String,
    name: String,
    kind: NewItemKindWire,
    template_path: Option<String>,
    rejected: Option<RejectedNewItemWire>,
}

impl From<&NewItemPlan> for NewItemPlanWire {
    fn from(p: &NewItemPlan) -> Self {
        NewItemPlanWire {
            directory: p.directory.clone(),
            name: p.name.clone(),
            kind: match p.kind {
                NewItemKind::Folder => NewItemKindWire::Folder,
                NewItemKind::File => NewItemKindWire::File,
            },
            template_path: p.template_path.clone(),
            rejected: p.rejected.as_ref().map(|r| RejectedNewItemWire {
                reason: match r.reason {
                    NewItemRejection::InvalidName => NewItemRejectionWire::InvalidName,
                    NewItemRejection::NameTaken => NewItemRejectionWire::NameTaken,
                    NewItemRejection::ParentMissing => NewItemRejectionWire::ParentMissing,
                    NewItemRejection::TemplateMissing => NewItemRejectionWire::TemplateMissing,
                },
                message: r.message.clone(),
            }),
        }
    }
}

impl From<NewItemPlanWire> for NewItemPlan {
    fn from(w: NewItemPlanWire) -> Self {
        NewItemPlan {
            directory: w.directory,
            name: w.name,
            kind: match w.kind {
                NewItemKindWire::Folder => NewItemKind::Folder,
                NewItemKindWire::File => NewItemKind::File,
            },
            template_path: w.template_path,
            rejected: w.rejected.map(|r| {
                RejectedNewItem::new(
                    match r.reason {
                        NewItemRejectionWire::InvalidName => NewItemRejection::InvalidName,
                        NewItemRejectionWire::NameTaken => NewItemRejection::NameTaken,
                        NewItemRejectionWire::ParentMissing => NewItemRejection::ParentMissing,
                        NewItemRejectionWire::TemplateMissing => NewItemRejection::TemplateMissing,
                    },
                    r.message,
                )
            }),
        }
    }
}

wire!(NewItemPlan => NewItemPlanWire);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FailedNewItemWire {
    message: String,
    access_denied: bool,
}

impl From<&FailedNewItem> for FailedNewItemWire {
    fn from(f: &FailedNewItem) -> Self {
        FailedNewItemWire {
            message: f.message.clone(),
            access_denied: f.access_denied,
        }
    }
}

impl From<FailedNewItemWire> for FailedNewItem {
    fn from(w: FailedNewItemWire) -> Self {
        FailedNewItem {
            message: w.message,
            access_denied: w.access_denied,
        }
    }
}

wire!(FailedNewItem => FailedNewItemWire);
