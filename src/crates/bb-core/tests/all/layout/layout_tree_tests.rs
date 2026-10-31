// Port of tests/BertBrowser.Core.Tests/LayoutTreeTests.cs: the pane layout algebra. Anything wrong
// here shows up as a layout that quietly degenerates after a few splits and closes.
use bb_core::layout::SplitOrientation::{self, Horizontal, Vertical};
use bb_core::layout::{LayoutTree, NodeId};

fn names(tree: &LayoutTree<String>) -> Vec<String> {
    tree.leaves()
        .iter()
        .map(|&l| tree.value(l).clone())
        .collect()
}

fn s(v: &str) -> String {
    v.to_string()
}

// --- Split ---

#[test]
fn split_lone_leaf_becomes_split_of_two_equal_panes() {
    for orientation in [Vertical, Horizontal] {
        let (mut tree, a) = LayoutTree::new(s("a"));

        let b = tree.split(a, orientation, s("b"));

        let root = tree.root();
        assert!(tree.is_split(root));
        assert_eq!(Some(orientation), tree.orientation(root));
        assert_eq!(&[a, b], tree.children(root));
        assert_eq!(tree.weight(a), tree.weight(b));
        assert!(tree.weight(a) > 0.0);
    }
}

#[test]
fn split_new_pane_goes_immediately_after_the_one_it_split() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    tree.split(a, Vertical, s("b"));
    tree.split(a, Vertical, s("c"));

    assert_eq!(vec!["a", "c", "b"], names(&tree));
}

#[test]
fn split_same_orientation_as_parent_adds_sibling_instead_of_nesting() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));

    tree.split(b, Vertical, s("c"));

    let root = tree.root();
    assert_eq!(3, tree.children(root).len());
    assert!(tree.children(root).iter().all(|&c| tree.is_leaf(c)));
}

#[test]
fn split_different_orientation_from_parent_nests_and_leaves_sibling_count_alone() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));

    tree.split(b, Horizontal, s("c"));

    let root = tree.root();
    assert_eq!(2, tree.children(root).len());
    let nested = tree.children(root)[1];
    assert!(tree.is_split(nested));
    assert_eq!(Some(Horizontal), tree.orientation(nested));
    assert_eq!(vec!["a", "b", "c"], names(&tree));
}

#[test]
fn split_takes_half_of_the_targets_space_and_leaves_siblings_alone() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    tree.set_weight(a, 3.0);
    tree.set_weight(b, 1.0);

    let c = tree.split(a, Vertical, s("c"));

    assert_eq!(1.5, tree.weight(a));
    assert_eq!(1.5, tree.weight(c));
    assert_eq!(1.0, tree.weight(b)); // the pane that was not split must not move
}

// --- Close ---

#[test]
fn close_last_remaining_pane_is_refused() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    assert!(!tree.close(a));
}

#[test]
fn close_collapses_a_split_left_with_one_child() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));

    assert!(tree.close(b));

    assert_eq!(a, tree.root());
}

#[test]
fn close_gives_the_freed_space_to_the_neighbour() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Vertical, s("c"));
    tree.set_weight(a, 2.0);
    tree.set_weight(b, 1.0);
    tree.set_weight(c, 1.0);

    assert!(tree.close(c));

    assert_eq!(vec!["a", "b"], names(&tree));
    assert_eq!(2.0, tree.weight(b)); // the share of c went to the pane beside it
    assert_eq!(2.0, tree.weight(a)); // and nowhere else
}

#[test]
fn close_flattens_a_nested_split_that_ends_up_matching_its_parent() {
    // a | (b over (c | d)): closing b leaves a same-orientation split inside a split.
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Horizontal, s("c"));
    tree.split(c, Vertical, s("d"));

    assert!(tree.close(b));

    let root = tree.root();
    assert!(tree.is_split(root));
    assert_eq!(Some(Vertical), tree.orientation(root));
    assert_eq!(vec!["a", "c", "d"], names(&tree));
    assert!(tree.children(root).iter().all(|&ch| tree.is_leaf(ch)));
}

// --- Traversal ---

#[test]
fn leaves_are_in_document_order() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    tree.split(b, Horizontal, s("c"));
    tree.split(a, Vertical, s("d"));

    assert_eq!(vec!["a", "d", "b", "c"], names(&tree));
}

#[test]
fn next_leaf_wraps_in_both_directions() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Vertical, s("c"));

    assert_eq!(b, tree.next_leaf(a, 1));
    assert_eq!(a, tree.next_leaf(c, 1));
    assert_eq!(c, tree.next_leaf(a, -1));
}

#[test]
fn find_leaf_locates_by_value() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));

    assert_eq!(Some(b), tree.find_leaf(|v| v == "b"));
    assert_eq!(None, tree.find_leaf(|v| v == "nope"));
}

// --- Swap and equalise ---

#[test]
fn swap_exchanges_two_siblings_and_their_shares() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    tree.set_weight(a, 3.0);
    tree.set_weight(b, 1.0);

    tree.swap(a, b);

    assert_eq!(vec!["b", "a"], names(&tree));
    // The left slot is still the wide one: the panes moved, the layout did not.
    assert_eq!(3.0, tree.weight(b));
    assert_eq!(1.0, tree.weight(a));
}

#[test]
fn swap_reaches_across_nested_splits() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Horizontal, s("c"));

    tree.swap(a, c);

    assert_eq!(vec!["c", "b", "a"], names(&tree));
    assert_well_formed(&tree, tree.root(), None);
}

#[test]
fn swap_with_itself_or_as_the_only_pane_changes_nothing() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    tree.swap(a, a);
    assert_eq!(a, tree.root());

    tree.split(a, Vertical, s("b"));
    tree.swap(a, a);
    assert_eq!(vec!["a", "b"], names(&tree));
}

#[test]
fn swap_twice_puts_everything_back() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Horizontal, s("c"));
    tree.set_weight(a, 5.0);

    tree.swap(a, c);
    tree.swap(a, c);

    assert_eq!(vec!["a", "b", "c"], names(&tree));
    assert_eq!(5.0, tree.weight(a));
}

#[test]
fn equalise_gives_every_sibling_the_same_share_at_every_level() {
    let (mut tree, a) = LayoutTree::new(s("a"));
    let b = tree.split(a, Vertical, s("b"));
    let c = tree.split(b, Horizontal, s("c"));
    tree.set_weight(a, 7.0);
    tree.set_weight(b, 2.0);
    tree.set_weight(c, 9.0);

    tree.equalise();

    let root = tree.root();
    assert!(tree.is_split(root));
    assert!(tree.children(root).iter().all(|&ch| tree.weight(ch) == 1.0));
    assert_eq!(tree.weight(b), tree.weight(c));
    assert_well_formed(&tree, root, None);
}

// --- Property test: random split/close sequences never degenerate ---

/// A small deterministic generator, so the sequences are the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

#[test]
fn random_splits_and_closes_preserve_every_invariant() {
    for seed in 1..=5u64 {
        let mut random = Lcg(seed);
        let (mut tree, _first) = LayoutTree::new(s("p0"));
        let mut next = 1;
        let mut expected = 1;

        for _step in 0..200 {
            let leaves = tree.leaves();
            let target = leaves[random.next(leaves.len())];

            if leaves.len() > 1 && random.next(3) == 0 {
                assert!(tree.close(target));
                expected -= 1;
            } else {
                let orientation = if random.next(2) == 0 {
                    Vertical
                } else {
                    Horizontal
                };
                tree.split(target, orientation, format!("p{next}"));
                next += 1;
                expected += 1;
            }

            assert_eq!(expected, tree.leaves().len(), "seed {seed}");
            assert_well_formed(&tree, tree.root(), None);
            let mut distinct = names(&tree);
            distinct.sort();
            distinct.dedup();
            assert_eq!(distinct.len(), tree.leaves().len(), "seed {seed}");
        }
    }
}

/// Every split holds at least two children, no split is nested directly inside a split running the
/// same way, and no pane has a zero or negative share.
fn assert_well_formed(
    tree: &LayoutTree<String>,
    node: NodeId,
    parent_orientation: Option<SplitOrientation>,
) {
    assert!(
        tree.weight(node) > 0.0,
        "non-positive weight {}",
        tree.weight(node)
    );
    if tree.is_leaf(node) {
        return;
    }
    assert!(
        tree.children(node).len() >= 2,
        "split with {} child(ren)",
        tree.children(node).len()
    );
    assert_ne!(parent_orientation, tree.orientation(node));
    for &child in tree.children(node) {
        assert_well_formed(tree, child, tree.orientation(node));
    }
}
