//! The pane layout: the live tree algebra and its serialisable mirror.
//! Spec: Core/Layout/LayoutTree.cs, Core/Layout/SessionLayout.cs.

mod session;
mod tree;

pub use session::{SessionLayout, SessionLayoutRules, SessionTab};
pub use tree::{LayoutTree, NodeId};

use serde::{Deserialize, Serialize};

/// How a split arranges its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitOrientation {
    /// Children sit side by side, separated by vertical splitter bars.
    Vertical,
    /// Children are stacked, separated by horizontal splitter bars.
    Horizontal,
}
