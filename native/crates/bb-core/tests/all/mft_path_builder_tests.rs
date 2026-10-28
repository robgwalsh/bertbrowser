// Port of tests/BertBrowser.Core.Tests/MftPathBuilderTests.cs.
use std::collections::HashMap;

use bb_core::attributes::HIDDEN;
use bb_core::mft::{DirCache, MftNode, MftPathBuilder};

const ROOT: u64 = MftPathBuilder::NTFS_ROOT;

fn tree() -> HashMap<u64, MftNode> {
    HashMap::from([
        (10, MftNode::new("Users", ROOT, true, 0)),
        (11, MftNode::new("Rob", 10, true, 0)),
        (12, MftNode::new("report.txt", 11, false, 0)),
    ])
}

#[test]
fn resolve_builds_full_path_from_parent_chain() {
    let mut cache = DirCache::new();

    let (path, hidden) =
        MftPathBuilder::try_resolve(&tree(), 12, r"C:\", &mut cache, ROOT).unwrap();

    assert_eq!(r"C:\Users\Rob\report.txt", path);
    assert!(!hidden);
}

#[test]
fn resolve_direct_child_of_root() {
    let map = HashMap::from([(20, MftNode::new("pagefile.sys", ROOT, false, HIDDEN))]);

    let (path, hidden) =
        MftPathBuilder::try_resolve(&map, 20, r"C:\", &mut DirCache::new(), ROOT).unwrap();

    assert_eq!(r"C:\pagefile.sys", path);
    assert!(hidden);
}

#[test]
fn resolve_inherits_hidden_from_ancestor() {
    let mut map = tree();
    map.get_mut(&11).unwrap().attributes = HIDDEN; // hide "Rob"

    let (_, hidden) =
        MftPathBuilder::try_resolve(&map, 12, r"C:\", &mut DirCache::new(), ROOT).unwrap();

    assert!(hidden); // file itself is not hidden, but its ancestor is
}

#[test]
fn resolve_broken_chain_fails() {
    let map = HashMap::from([(
        12,
        MftNode::new("orphan.txt", 999 /* missing parent */, false, 0),
    )]);

    assert!(MftPathBuilder::try_resolve(&map, 12, r"C:\", &mut DirCache::new(), ROOT).is_none());
}

#[test]
fn resolve_populates_directory_cache_for_ancestors() {
    let mut cache = DirCache::new();

    MftPathBuilder::try_resolve(&tree(), 12, r"C:\", &mut cache, ROOT);

    assert_eq!(r"C:\Users", cache[&10].path);
    assert_eq!(r"C:\Users\Rob", cache[&11].path);
    assert!(!cache.contains_key(&12)); // the file itself is not a directory
}
