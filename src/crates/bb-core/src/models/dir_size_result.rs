//! A cached recursive directory total. Spec: Core/Models/DirSizeResult.cs. The type itself is
//! defined beside the repository that fills it (`crate::data::dir_size`), which was ported first; it
//! is re-exported here so the models module names everything the C# `Models` namespace did.

pub use crate::data::DirSizeResult;
