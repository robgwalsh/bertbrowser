//! Reads and writes `BenchResultSet` files: camelCase, indented, nulls omitted, because the files are
//! committed as baselines and read in diffs, so they are formatted for a person first. A file from
//! a different schema version is refused rather than half-read — a silently missing field would
//! compare as "no regression". Spec: Core/Benchmarking/BenchResultsJson.cs.

use std::path::Path;

use super::{BenchError, BenchResultSet, CURRENT_SCHEMA_VERSION};

pub struct BenchResultsJson;

impl BenchResultsJson {
    pub fn serialize(set: &BenchResultSet) -> String {
        serde_json::to_string_pretty(set).expect("a results set always serializes")
    }

    pub fn deserialize(json: &str) -> Result<BenchResultSet, BenchError> {
        let set: Option<BenchResultSet> =
            serde_json::from_str(json).map_err(|e| BenchError::NotResultsFile(e.to_string()))?;
        let Some(set) = set else {
            return Err(BenchError::NotResultsFile("empty document.".to_string()));
        };
        if set.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(BenchError::SchemaVersion(set.schema_version));
        }
        Ok(set)
    }

    pub fn write(set: &BenchResultSet, path: &Path) -> Result<(), BenchError> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, Self::serialize(set) + "\n")?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<BenchResultSet, BenchError> {
        Self::deserialize(&std::fs::read_to_string(path)?)
    }
}
