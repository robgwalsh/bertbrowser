//! The pane layout algebra: split a pane in two, close one, swap, equalise, and enumerate what is
//! left. Pure and UI-free so the invariants that keep the layout sane (no split with fewer than two
//! children, no split nested directly inside a split of the same orientation, at least one pane
//! always open) are tested rather than eyeballed. Nodes live in an arena and are addressed by
//! `NodeId`, which stays valid for a leaf across every operation until it is closed.

use super::SplitOrientation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

enum Node<T> {
    Leaf {
        value: T,
        weight: f64,
    },
    Split {
        orientation: SplitOrientation,
        children: Vec<NodeId>,
        weight: f64,
    },
}

pub struct LayoutTree<T> {
    nodes: Vec<Option<Node<T>>>,
    root: NodeId,
}

impl<T> LayoutTree<T> {
    /// A layout of one pane.
    pub fn new(value: T) -> (LayoutTree<T>, NodeId) {
        let tree = LayoutTree {
            nodes: vec![Some(Node::Leaf { value, weight: 1.0 })],
            root: NodeId(0),
        };
        (tree, NodeId(0))
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn is_leaf(&self, id: NodeId) -> bool {
        matches!(self.node(id), Node::Leaf { .. })
    }

    pub fn is_split(&self, id: NodeId) -> bool {
        !self.is_leaf(id)
    }

    pub fn value(&self, id: NodeId) -> &T {
        match self.node(id) {
            Node::Leaf { value, .. } => value,
            Node::Split { .. } => panic!("not a leaf"),
        }
    }

    pub fn value_mut(&mut self, id: NodeId) -> &mut T {
        match self.node_mut(id) {
            Node::Leaf { value, .. } => value,
            Node::Split { .. } => panic!("not a leaf"),
        }
    }

    /// This node's share of its parent's space, as a star weight. Always > 0.
    pub fn weight(&self, id: NodeId) -> f64 {
        match self.node(id) {
            Node::Leaf { weight, .. } | Node::Split { weight, .. } => *weight,
        }
    }

    pub fn set_weight(&mut self, id: NodeId, w: f64) {
        match self.node_mut(id) {
            Node::Leaf { weight, .. } | Node::Split { weight, .. } => *weight = w,
        }
    }

    pub fn orientation(&self, id: NodeId) -> Option<SplitOrientation> {
        match self.node(id) {
            Node::Split { orientation, .. } => Some(*orientation),
            Node::Leaf { .. } => None,
        }
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        match self.node(id) {
            Node::Split { children, .. } => children,
            Node::Leaf { .. } => &[],
        }
    }

    /// Splits `target`, putting a new pane beside (or below) it, and returns the new pane. The new
    /// pane takes half of the target's space, so the rest of the layout does not move. Splitting
    /// inside a parent that already runs the same way appends a sibling rather than nesting.
    pub fn split(&mut self, target: NodeId, orientation: SplitOrientation, value: T) -> NodeId {
        let half = self.weight(target) / 2.0;
        self.set_weight(target, half);
        let inserted = self.push(Node::Leaf {
            value,
            weight: half,
        });

        let parent = self.parent_of(target);
        if let Some(p) = parent
            && self.orientation(p) == Some(orientation)
        {
            let index = self.index_in(p, target);
            self.children_mut(p).insert(index + 1, inserted);
            return inserted;
        }

        // The new split stands exactly where the target did, so it inherits the target's share.
        self.set_weight(target, 1.0);
        self.set_weight(inserted, 1.0);
        let split = self.push(Node::Split {
            orientation,
            children: vec![target, inserted],
            weight: half * 2.0,
        });
        match parent {
            None => self.root = split,
            Some(p) => {
                let index = self.index_in(p, target);
                self.children_mut(p)[index] = split;
            }
        }
        inserted
    }

    /// Removes `target`. False when it is the last pane (closing the only pane is refused) or not in
    /// this tree.
    pub fn close(&mut self, target: NodeId) -> bool {
        if target == self.root {
            return false;
        }
        let Some(parent) = self.parent_of(target) else {
            return false;
        };
        let index = self.index_in(parent, target);
        let freed = self.weight(target);
        self.children_mut(parent).remove(index);
        self.nodes[target.0] = None;

        // The freed space goes to whichever neighbour the closed pane was touching.
        let count = self.children(parent).len();
        let neighbour = self.children(parent)[index.min(count - 1)];
        let w = self.weight(neighbour);
        self.set_weight(neighbour, w + freed);

        if count > 1 {
            self.normalize_root();
            return true;
        }

        // A split with one child is not a split any more: hoist that child into its place.
        let survivor = self.children(parent)[0];
        self.set_weight(survivor, self.weight(parent));
        match self.parent_of(parent) {
            None => self.root = survivor,
            Some(g) => {
                let i = self.index_in(g, parent);
                self.children_mut(g)[i] = survivor;
            }
        }
        self.nodes[parent.0] = None;
        self.normalize_root();
        true
    }

    /// Exchanges two panes' places, each taking the other's share of the space, so the shape of the
    /// layout stays put and only what is in it moves. The leaves themselves change places rather
    /// than their values: a leaf is what the view keeps a pane's control against.
    pub fn swap(&mut self, first: NodeId, second: NodeId) {
        if first == second {
            return;
        }
        let (Some(fp), Some(sp)) = (self.parent_of(first), self.parent_of(second)) else {
            return;
        };
        let fi = self.index_in(fp, first);
        let si = self.index_in(sp, second);
        self.children_mut(fp)[fi] = second;
        self.children_mut(sp)[si] = first;
        let (fw, sw) = (self.weight(first), self.weight(second));
        self.set_weight(first, sw);
        self.set_weight(second, fw);
    }

    /// Gives every pane in a split the same share, at every level.
    pub fn equalise(&mut self) {
        self.equalise_under(self.root);
    }

    fn equalise_under(&mut self, id: NodeId) {
        for child in self.children(id).to_vec() {
            self.set_weight(child, 1.0);
            self.equalise_under(child);
        }
    }

    /// Every pane, in the order they appear on screen (left to right, top to bottom). This order is
    /// what "focus the next pane" walks.
    pub fn leaves(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_leaves(self.root, &mut out);
        out
    }

    fn collect_leaves(&self, id: NodeId, out: &mut Vec<NodeId>) {
        match self.node(id) {
            Node::Leaf { .. } => out.push(id),
            Node::Split { children, .. } => {
                for &c in children {
                    self.collect_leaves(c, out);
                }
            }
        }
    }

    pub fn find_leaf(&self, mut pred: impl FnMut(&T) -> bool) -> Option<NodeId> {
        self.leaves().into_iter().find(|&id| pred(self.value(id)))
    }

    /// The pane `step` positions after `from` in document order, wrapping around. `step` may be
    /// negative.
    pub fn next_leaf(&self, from: NodeId, step: i64) -> NodeId {
        let leaves = self.leaves();
        if leaves.is_empty() {
            return from;
        }
        let Some(index) = leaves.iter().position(|&l| l == from) else {
            return leaves[0];
        };
        let n = leaves.len() as i64;
        leaves[(((index as i64 + step) % n + n) % n) as usize]
    }

    pub fn parent_of(&self, child: NodeId) -> Option<NodeId> {
        self.find_parent(self.root, child)
    }

    fn find_parent(&self, root: NodeId, child: NodeId) -> Option<NodeId> {
        let Node::Split { children, .. } = self.node(root) else {
            return None;
        };
        if children.contains(&child) {
            return Some(root);
        }
        children.iter().find_map(|&c| self.find_parent(c, child))
    }

    /// Folds a split into its parent when the two run the same way, which can happen once a close
    /// leaves a nested split with a single sibling.
    fn normalize_root(&mut self) {
        self.root = self.normalize(self.root);
    }

    fn normalize(&mut self, id: NodeId) -> NodeId {
        if self.is_leaf(id) {
            return id;
        }
        let orientation = self.orientation(id).expect("split");
        let mut i = 0;
        while i < self.children(id).len() {
            let child = self.children(id)[i];
            let child = self.normalize(child);
            self.children_mut(id)[i] = child;
            if self.orientation(child) != Some(orientation) {
                i += 1;
                continue;
            }
            // Redistribute the nested split's share across the children being hoisted.
            let grandchildren = self.children(child).to_vec();
            let mut total: f64 = grandchildren.iter().map(|&g| self.weight(g)).sum();
            if total <= 0.0 {
                total = grandchildren.len() as f64;
            }
            let nested_weight = self.weight(child);
            for &g in &grandchildren {
                let w = self.weight(g);
                self.set_weight(g, nested_weight * w / total);
            }
            let kids = self.children_mut(id);
            kids.remove(i);
            for (k, g) in grandchildren.iter().enumerate() {
                kids.insert(i + k, *g);
            }
            self.nodes[child.0] = None;
            i += grandchildren.len();
        }
        if self.children(id).len() == 1 {
            let survivor = self.children(id)[0];
            self.set_weight(survivor, self.weight(id));
            self.nodes[id.0] = None;
            return survivor;
        }
        id
    }

    fn push(&mut self, node: Node<T>) -> NodeId {
        if let Some(i) = self.nodes.iter().position(Option::is_none) {
            self.nodes[i] = Some(node);
            return NodeId(i);
        }
        self.nodes.push(Some(node));
        NodeId(self.nodes.len() - 1)
    }

    fn node(&self, id: NodeId) -> &Node<T> {
        self.nodes[id.0].as_ref().expect("live node")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node<T> {
        self.nodes[id.0].as_mut().expect("live node")
    }

    fn children_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        match self.node_mut(id) {
            Node::Split { children, .. } => children,
            Node::Leaf { .. } => panic!("not a split"),
        }
    }

    fn index_in(&self, parent: NodeId, child: NodeId) -> usize {
        self.children(parent)
            .iter()
            .position(|&c| c == child)
            .expect("child of parent")
    }
}
