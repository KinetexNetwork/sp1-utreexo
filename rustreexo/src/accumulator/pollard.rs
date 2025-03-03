//! A full Pollard accumulator implementation. This is a simple version of the forest,
//! that keeps every node in memory. This is may require more memory, but is faster
//! to update, prove and verify.
//!
//! # Example
//! ```
//! use rustreexo::accumulator::node_hash::BitcoinNodeHash;
//! use rustreexo::accumulator::pollard::Pollard;
//! let values = vec![0, 1, 2, 3, 4, 5, 6, 7];
//! let hashes: Vec<BitcoinNodeHash> = values
//!     .into_iter()
//!     .map(|i| BitcoinNodeHash::from([i; 32]))
//!     .collect();
//!
//! let mut p = Pollard::new();
//!
//! p.modify(&hashes, &[]).expect("Pollard should not fail");
//! assert_eq!(p.get_roots().len(), 1);
//!
//! p.modify(&[], &hashes).expect("Still should not fail"); // Remove leaves from the accumulator
//!
//! assert_eq!(p.get_roots().len(), 1);
//! assert_eq!(p.get_roots()[0].get_data(), BitcoinNodeHash::default());
//! ```

use core::fmt;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io::Read;
use std::io::Write;
use std::rc::Rc;
use std::rc::Weak;

use serde::Deserialize;
use serde::Serialize;

use super::node_hash::BitcoinNodeHash;
use super::proof::Proof;
use super::util::detect_offset;
use super::util::get_proof_positions;
use super::util::is_left_niece;
use super::util::is_root_populated;
use super::util::left_child;
use super::util::max_position_at_row;
use super::util::right_child;
use super::util::root_position;
use super::util::tree_rows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum NodeType {
    Branch,
    Leaf,
    BranchLeftOnly,
    BranchRightOnly,
    BranchNoChildren,
}

/// A forest node that can either be a leaf or a branch.
#[derive(Clone)]
pub struct Node {
    /// The type of this node.
    ty: NodeType,
    /// The hash of the stored in this node.
    data: Cell<BitcoinNodeHash>,
    /// The parent of this node, if any.
    parent: RefCell<Option<Weak<Node>>>,
    /// The left and right children of this node, if any.
    left: RefCell<Option<Rc<Node>>>,
    /// The left and right children of this node, if any.
    right: RefCell<Option<Rc<Node>>>,
    /// Helper flag for zkutreexo project. Shows if node was used in utreexo mutations.
    pub used: Cell<bool>,
}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.data.get().cmp(&other.data.get())
    }
}

impl std::hash::Hash for Node {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.get().hash(state);
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.data.get() == other.data.get()
    }
}

impl Eq for Node {
    fn assert_receiver_is_total_eq(&self) {
        self.data.get().assert_receiver_is_total_eq();
    }
}

use std::io::Cursor;

use serde::de::Deserializer;
use serde::de::Visitor;
use serde::de::{self};
use serde::ser::Serializer;

impl Serialize for Pollard {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Create a buffer to hold the serialized bytes
        let mut buffer = Vec::new();

        // Use your existing serialize method to write into the buffer
        self.serialize(&mut buffer)
            .map_err(|err| serde::ser::Error::custom(err.to_string()))?;

        // Depending on the format, serialize the bytes appropriately

        // For binary formats (e.g., bincode)
        serializer.serialize_bytes(&buffer)

        // For text-based formats (e.g., JSON), you might need to encode the bytes
        // as a Base64 string or hexadecimal string. Uncomment the following lines
        // if you're using a text-based serializer:

        // use base64::encode;
        // let encoded = encode(&buffer);
        // serializer.serialize_str(&encoded)
    }
}

impl<'de> Deserialize<'de> for Pollard {
    fn deserialize<D>(deserializer: D) -> Result<Pollard, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PollardVisitor;

        impl Visitor<'_> for PollardVisitor {
            type Value = Pollard;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a byte array representing Pollard")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Pollard, E>
            where
                E: de::Error,
            {
                let mut reader = Cursor::new(v);
                Pollard::deserialize(&mut reader).map_err(|err| de::Error::custom(err.to_string()))
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Pollard, E>
            where
                E: de::Error,
            {
                let mut reader = Cursor::new(v);
                Pollard::deserialize(&mut reader).map_err(|err| de::Error::custom(err.to_string()))
            }
        }
        deserializer.deserialize_bytes(PollardVisitor)
    }
}

impl Node {
    /// Recomputes the hash of all nodes, up to the root.
    fn recompute_hashes(&self) {
        let left = self.left.borrow();
        let right = self.right.borrow();

        if let (Some(left), Some(right)) = (left.as_deref(), right.as_deref()) {
            self.data.replace(BitcoinNodeHash::parent_hash(
                &left.data.get(),
                &right.data.get(),
            ));
        }
        if let Some(ref parent) = *self.parent.borrow() {
            if let Some(p) = parent.upgrade() {
                p.recompute_hashes();
            }
        }
    }
    pub fn restore_used_flag(&self) {
        self.used.set(false);
        if let Some(left) = self.left.borrow().as_ref() {
            left.restore_used_flag();
        }
        if let Some(right) = self.right.borrow().as_ref() {
            right.restore_used_flag();
        }
    }
    /// Strips all unused nodes from the tree.
    pub fn strip_unused(&mut self) {
        if let Some(left) = self.left.take() {
            if !left.used.get() {
                self.left = RefCell::new(None);
            } else {
                let mut new_left = left.as_ref().clone();
                new_left.strip_unused();
                self.left = RefCell::new(Some(Rc::new(new_left)));
            }
        }
        if let Some(right) = self.right.take() {
            if !right.used.get() {
                self.right = RefCell::new(None);
            } else {
                let mut new_right = right.as_ref().clone();
                new_right.strip_unused();
                self.right = RefCell::new(Some(Rc::new(new_right)));
            }
        }
    }
    /// Writes one node to the writer, this method will recursively write all children.
    /// The primary use of this method is to serialize the accumulator. In this case,
    /// you should call this method on each root in the forest.
    pub fn write_one<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut self_copy = self.clone();

        if self.ty == NodeType::Branch {
            if self.left.borrow().is_none() {
                self_copy.ty = NodeType::BranchRightOnly;
            }
            if self.right.borrow().is_none() {
                self_copy.ty = NodeType::BranchLeftOnly;
            }
            if self.left.borrow().is_none() && self.right.borrow().is_none() {
                self_copy.ty = NodeType::BranchNoChildren;
            }
        }

        match self_copy.ty {
            NodeType::Branch => writer.write_all(&0_u64.to_le_bytes())?,
            NodeType::Leaf => writer.write_all(&1_u64.to_le_bytes())?,
            NodeType::BranchLeftOnly => writer.write_all(&2_u64.to_le_bytes())?,
            NodeType::BranchRightOnly => writer.write_all(&3_u64.to_le_bytes())?,
            NodeType::BranchNoChildren => writer.write_all(&4_u64.to_le_bytes())?,
        }
        self_copy.data.get().write(writer)?;

        if self_copy.ty != NodeType::BranchRightOnly {
            self.left
                .borrow()
                .as_ref()
                .map(|l| l.write_one(writer))
                .transpose()?;
        }

        if self_copy.ty != NodeType::BranchLeftOnly {
            self.right
                .borrow()
                .as_ref()
                .map(|r| r.write_one(writer))
                .transpose()?;
        }
        Ok(())
    }
    /// Reads one node from the reader, this method will recursively read all children.
    /// The primary use of this method is to deserialize the accumulator. In this case,
    /// you should call this method on each root in the forest, assuming you know how
    /// many roots there are.
    #[allow(clippy::type_complexity)]
    pub fn read_one<R: std::io::Read>(
        reader: &mut R,
    ) -> std::io::Result<(Rc<Node>, BTreeMap<BitcoinNodeHash, Weak<Node>>)> {
        fn _read_one<R: std::io::Read>(
            ancestor: Option<Rc<Node>>,
            reader: &mut R,
            index: &mut BTreeMap<BitcoinNodeHash, Weak<Node>>,
        ) -> std::io::Result<Rc<Node>> {
            let mut ty = [0u8; 8];
            reader.read_exact(&mut ty)?;
            let data = BitcoinNodeHash::read(reader)?;

            let ty = match u64::from_le_bytes(ty) {
                0 => NodeType::Branch,
                1 => NodeType::Leaf,
                2 => NodeType::BranchLeftOnly,
                3 => NodeType::BranchRightOnly,
                4 => NodeType::BranchNoChildren,
                _ => panic!("Invalid node type"),
            };
            if ty == NodeType::Leaf {
                let leaf = Rc::new(Node {
                    ty,
                    data: Cell::new(data),
                    parent: RefCell::new(ancestor.map(|a| Rc::downgrade(&a))),
                    left: RefCell::new(None),
                    right: RefCell::new(None),
                    used: Cell::new(false),
                });
                index.insert(leaf.data.get(), Rc::downgrade(&leaf));
                return Ok(leaf);
            }
            let node = Rc::new(Node {
                ty: NodeType::Branch,
                data: Cell::new(data),
                parent: RefCell::new(ancestor.map(|a| Rc::downgrade(&a))),
                left: RefCell::new(None),
                right: RefCell::new(None),
                used: Cell::new(false),
            });
            if !data.is_empty() {
                if ty != NodeType::BranchRightOnly && ty != NodeType::BranchNoChildren {
                    let left = _read_one(Some(node.clone()), reader, index)?;
                    node.left.replace(Some(left));
                } else {
                    node.left.replace(None);
                }
                if ty != NodeType::BranchLeftOnly && ty != NodeType::BranchNoChildren {
                    let right = _read_one(Some(node.clone()), reader, index)?;

                    node.right.replace(Some(right));
                } else {
                    node.right.replace(None);
                }
            }

            if node.left.borrow().is_some() {
                node.left
                    .borrow()
                    .as_ref()
                    .map(|l| l.parent.replace(Some(Rc::downgrade(&node))));
            }
            if node.right.borrow().is_some() {
                node.right
                    .borrow()
                    .as_ref()
                    .map(|r| r.parent.replace(Some(Rc::downgrade(&node))));
            }
            Ok(node)
        }

        let mut index = BTreeMap::new();
        let root = _read_one(None, reader, &mut index)?;
        Ok((root, index))
    }
    /// Returns the data associated with this node.
    pub fn get_data(&self) -> BitcoinNodeHash {
        self.data.get()
    }
}

impl Debug for Node {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}{:02x}", self.data.get()[0], self.data.get()[1])
    }
}
/// The actual Pollard accumulator, it implements all methods required to update the forest
/// and to prove/verify membership.
#[derive(Default, Clone)]
pub struct Pollard {
    /// The roots of the forest, all leaves are children of these roots, and therefore
    /// owned by them.
    roots: Vec<Rc<Node>>,
    /// The number of leaves in the forest. Actually, this is the number of leaves we ever
    /// added to the forest.
    pub leaves: u64,
    /// A map of all nodes in the forest, indexed by their hash, this is used to lookup
    /// leaves when proving membership.
    map: BTreeMap<BitcoinNodeHash, Weak<Node>>,
}
impl Pollard {
    /// Creates a new empty [Pollard].
    /// # Example
    /// ```
    /// use rustreexo::accumulator::pollard::Pollard;
    /// let mut pollard = Pollard::new();
    /// ```
    pub fn new() -> Pollard {
        Pollard {
            map: BTreeMap::new(),
            roots: Vec::new(),
            leaves: 0,
        }
    }

    /// resets used flag for all nodes to false
    pub fn restore_used_flag(&mut self) {
        for wnode in self.map.values() {
            if let Some(rc_node) = wnode.upgrade() {
                rc_node.used.set(false);
            }
        }
        for root in &self.roots {
            root.restore_used_flag();
        }
    }

    /// Returns version of pollard where all unused nodes are removed
    pub fn get_stripped_pollard(&self) -> Pollard {
        for root in &self.roots {
            root.used.set(true);
        }
        let mut new_roots: Vec<Rc<Node>> = Default::default();
        for root in self.roots.iter() {
            if root.used.get() {
                let mut new_root = root.as_ref().clone();
                new_root.strip_unused();
                new_roots.push(Rc::new(new_root));
            }
        }
        let new_leaves: u64 = self.leaves; // TODO: Am I sure we don't need it??

        let mut new_map: BTreeMap<BitcoinNodeHash, Weak<Node>> = Default::default();

        for wnode in self.map.values() {
            if let Some(rc_node) = wnode.upgrade() {
                if rc_node.used.get() {
                    new_map.insert(rc_node.data.get(), Rc::downgrade(&rc_node));
                }
            }
        }
        Pollard {
            map: new_map,
            roots: new_roots,
            leaves: new_leaves,
        }
    }

    /// Returns a pollard with same nodes but flags setted according to modify
    pub fn fake_modify(&mut self, add: &[BitcoinNodeHash], del: &[BitcoinNodeHash]) -> Pollard {
        let new_pollard = self.clone();
        #[allow(clippy::mutable_key_type)]
        let link_map = self.link_pollards(&new_pollard);
        self.fake_del(del, &link_map);
        self.fake_add(add, &link_map);
        new_pollard
    }

    #[allow(clippy::mutable_key_type)]
    fn fake_add(&mut self, values: &[BitcoinNodeHash], link_map: &BTreeMap<Node, Weak<Node>>) {
        for value in values {
            self.fake_add_single(*value, link_map);
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn fake_add_single(&mut self, value: BitcoinNodeHash, link_map: &BTreeMap<Node, Weak<Node>>) {
        let mut node: Rc<Node> = Rc::new(Node {
            ty: NodeType::Leaf,
            parent: RefCell::new(None),
            data: Cell::new(value),
            left: RefCell::new(None),
            right: RefCell::new(None),
            used: Cell::new(true),
        });
        self.map.insert(value, Rc::downgrade(&node));
        let mut leaves = self.leaves;
        while leaves & 1 != 0 {
            let root = self.roots.pop().unwrap();
            root.used.set(true);
            if let Some(root_link) = link_map.get(&root) {
                root_link.upgrade().unwrap().used.set(true);
            }
            if root.get_data() == BitcoinNodeHash::empty() {
                leaves >>= 1;
                continue;
            }
            let new_node = Rc::new(Node {
                ty: NodeType::Branch,
                parent: RefCell::new(None),
                data: Cell::new(BitcoinNodeHash::parent_hash(
                    &root.data.get(),
                    &node.data.get(),
                )),
                left: RefCell::new(Some(root.clone())),
                right: RefCell::new(Some(node.clone())),
                used: Cell::new(true),
            });
            root.parent.replace(Some(Rc::downgrade(&new_node)));
            root.parent
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .used
                .set(true);

            if let Some(root_link) = link_map.get(&root) {
                root_link.upgrade().unwrap().used.set(true);
            }

            node.parent.replace(Some(Rc::downgrade(&new_node)));
            node.parent
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .used
                .set(true);

            if let Some(node_link) = link_map.get(&node) {
                node_link.upgrade().unwrap().used.set(true);
            }

            node = new_node;
            leaves >>= 1;
        }
        self.roots.push(node);
        self.leaves += 1;
    }

    #[allow(clippy::mutable_key_type)]
    fn fake_del(&mut self, targets: &[BitcoinNodeHash], link_map: &BTreeMap<Node, Weak<Node>>) {
        let mut pos = targets
            .iter()
            .flat_map(|target| self.map.get(target))
            .flat_map(|target| target.upgrade())
            .map(|target| {
                (
                    self.fake_get_pos(self.map.get(&target.data.get()).unwrap()),
                    target.data.get(),
                )
            })
            .collect::<Vec<_>>();

        pos.sort();
        let (_, targets): (Vec<u64>, Vec<BitcoinNodeHash>) = pos.into_iter().unzip();
        for target in targets {
            if let Some(target) = self.map.remove(&target) {
                let mut tgt = target.upgrade().unwrap().clone().as_ref().clone();
                self.fake_del_single(&mut tgt, link_map);
            }
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn fake_del_single(
        &mut self,
        node: &mut Node,
        link_map: &BTreeMap<Node, Weak<Node>>,
    ) -> Option<()> {
        let parent = node.parent.borrow();
        // Deleting a root
        let parent = match *parent {
            Some(ref node) => node.upgrade()?,
            None => {
                let pos = self.roots.iter().position(|x| x.data == node.data).unwrap();
                self.roots[pos] = Rc::new(Node {
                    ty: NodeType::Branch,
                    parent: RefCell::new(None),
                    data: Cell::new(BitcoinNodeHash::default()),
                    left: RefCell::new(None),
                    right: RefCell::new(None),
                    used: Cell::new(true),
                });
                return None;
            }
        };
        parent.used.set(true);
        if let Some(parent_link) = link_map.get(&parent) {
            parent_link.upgrade().unwrap().used.set(true);
        }
        let me = parent.left.borrow();
        // Can unwrap because we know the sibling exists
        let sibling = if me.as_deref()?.data == node.data {
            parent.right.borrow().clone()
        } else {
            parent.left.borrow().clone()
        };
        if let Some(ref sibling) = sibling {
            sibling.used.set(true);
            if let Some(sibling_link) = link_map.get(sibling) {
                sibling_link.upgrade().unwrap().used.set(true);
            }
            let grandparent = parent.parent.borrow().clone();
            if let Some(ref gp) = grandparent {
                gp.upgrade().unwrap().used.set(true);
                let gp_owned = gp.upgrade().unwrap().clone().as_ref().clone();
                if let Some(gp_link) = link_map.get(&gp_owned) {
                    gp_link.upgrade().unwrap().used.set(true);
                }
            }

            sibling.parent.replace(grandparent.clone());
            if let Some(ref grandparent) = grandparent.and_then(|g| g.upgrade()) {
                if grandparent.left.borrow().clone().as_ref().unwrap().data == parent.data {
                    grandparent.left.borrow().clone().unwrap().used.set(true);
                    let gp_left_owned = grandparent
                        .left
                        .borrow()
                        .clone()
                        .unwrap()
                        .clone()
                        .as_ref()
                        .clone();
                    if let Some(gp_left_link) = link_map.get(&gp_left_owned) {
                        gp_left_link.upgrade().unwrap().used.set(true);
                    }
                    grandparent.left.replace(Some(sibling.clone()));
                } else {
                    grandparent.right.borrow().clone().unwrap().used.set(true);
                    let gp_right_owned = grandparent
                        .right
                        .borrow()
                        .clone()
                        .unwrap()
                        .clone()
                        .as_ref()
                        .clone();
                    if let Some(gp_right_link) = link_map.get(&gp_right_owned) {
                        gp_right_link.upgrade().unwrap().used.set(true);
                    }
                    grandparent.right.replace(Some(sibling.clone()));
                }
                sibling.recompute_hashes();
            } else {
                let pos = self
                    .roots
                    .iter()
                    .position(|x| x.data == parent.data)
                    .unwrap();
                self.roots[pos] = sibling.clone();
            }
        };

        Some(())
    }

    #[allow(clippy::mutable_key_type)]
    fn link_pollards(&self, other: &Pollard) -> BTreeMap<Node, Weak<Node>> {
        let mut res = BTreeMap::new();
        for (self_root, other_root) in self.roots.iter().zip(other.roots.iter()) {
            Pollard::link_pollards_inner(self_root, other_root, &mut res);
        }
        res
    }

    #[allow(clippy::mutable_key_type)]
    fn link_pollards_inner(
        first_root: &Rc<Node>,
        second_root: &Rc<Node>,
        res: &mut BTreeMap<Node, Weak<Node>>,
    ) {
        let first_node: Node = first_root.clone().as_ref().clone();
        res.insert(first_node, Rc::downgrade(second_root));

        if let Some(first_left) = first_root.left.borrow().as_ref() {
            if let Some(second_left) = second_root.left.borrow().as_ref() {
                Pollard::link_pollards_inner(first_left, second_left, res);
            }
        }

        if let Some(first_right) = first_root.right.borrow().as_ref() {
            if let Some(second_right) = second_root.right.borrow().as_ref() {
                Pollard::link_pollards_inner(first_right, second_right, res);
            }
        }
    }

    /// Writes the Pollard to a writer. Used to send the accumulator over the wire
    /// or to disk.
    /// # Example
    /// ```
    /// use rustreexo::accumulator::pollard::Pollard;
    ///
    /// let mut pollard = Pollard::new();
    /// let mut serialized = Vec::new();
    /// pollard.serialize(&mut serialized).unwrap();
    ///
    /// assert_eq!(
    ///     serialized,
    ///     vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    /// );
    /// ```
    pub fn serialize<W: Write>(&self, mut writer: W) -> std::io::Result<()> {
        writer.write_all(&self.leaves.to_le_bytes())?;
        writer.write_all(&self.roots.len().to_le_bytes())?;

        for root in &self.roots {
            root.write_one(&mut writer).unwrap();
        }

        Ok(())
    }
    /// Deserializes a pollard from a reader.
    /// # Example
    /// ```
    /// use std::io::Cursor;
    ///
    /// use rustreexo::accumulator::pollard::Pollard;
    /// let mut serialized = Cursor::new(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    /// let pollard = Pollard::deserialize(&mut serialized).unwrap();
    /// assert_eq!(pollard.leaves, 0);
    /// assert_eq!(pollard.get_roots().len(), 0);
    /// ```
    pub fn deserialize<R: Read>(mut reader: R) -> std::io::Result<Pollard> {
        fn read_u64<R: Read>(reader: &mut R) -> std::io::Result<u64> {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf)?;
            Ok(u64::from_le_bytes(buf))
        }
        let leaves = read_u64(&mut reader)?;
        let roots_len = read_u64(&mut reader)?;

        let mut roots = Vec::new();
        let mut map = BTreeMap::new();
        for _ in 0..roots_len {
            let (root, _map) = Node::read_one(&mut reader)?;
            map.extend(_map);
            roots.push(root);
        }
        Ok(Pollard { roots, leaves, map })
    }
    /// Returns the hash of a given position in the tree.
    fn get_hash(&self, pos: u64) -> Result<BitcoinNodeHash, String> {
        let (node, _, _) = self.grab_node(pos)?;
        Ok(node.data.get())
    }
    /// Proves that a given set of hashes is in the accumulator. It returns a proof
    /// and the hashes that we what to prove, but sorted by position in the tree.
    /// # Example
    /// ```
    /// use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    /// use rustreexo::accumulator::pollard::Pollard;
    /// let mut pollard = Pollard::new();
    /// let hashes = vec![0, 1, 2, 3, 4, 5, 6, 7]
    ///     .iter()
    ///     .map(|n| BitcoinNodeHash::from([*n; 32]))
    ///     .collect::<Vec<_>>();
    /// pollard.modify(&hashes, &[]).unwrap();
    /// // We want to prove that the first two hashes are in the accumulator.
    /// let proof = pollard.prove(&[hashes[1], hashes[0]]).unwrap();
    /// //TODO: Verify the proof
    /// ```
    pub fn prove(&self, targets: &[BitcoinNodeHash]) -> Result<Proof, String> {
        let mut positions = Vec::new();
        for target in targets {
            let node = self.map.get(target).ok_or("Could not find node")?;
            let position = self.get_pos(node);
            positions.push(position);
        }
        let needed = get_proof_positions(&positions, self.leaves, tree_rows(self.leaves));
        let proof = needed
            .iter()
            .map(|pos| self.get_hash(*pos).unwrap())
            .collect::<Vec<_>>();
        Ok(Proof::new(positions, proof))
    }

    /// Returns nothing and sets used flag to true for all nodes used in prove function.
    pub fn fake_prove(&mut self, targets: &[BitcoinNodeHash]) {
        let mut positions = Vec::new();
        for target in targets {
            let node = self.map.get(target).unwrap();
            node.upgrade().unwrap().used.set(true);
            let position = self.get_pos(node);
            positions.push(position);
        }
        let needed = get_proof_positions(&positions, self.leaves, tree_rows(self.leaves));
        // TODO: can we delete this?
        let _proof = needed
            .iter()
            .map(|pos| self.get_hash(*pos).unwrap())
            .collect::<Vec<_>>();
    }
    /// Returns a reference to the roots in this Pollard.
    pub fn get_roots(&self) -> &[Rc<Node>] {
        &self.roots
    }
    /// Modify is the main API to a [Pollard]. Because order matters, you can only `modify`
    /// a [Pollard], and internally it'll add and delete, in the correct order.
    ///
    /// This method accepts two vectors as parameter, a vec of [Hash] and a vec of [u64]. The
    /// first one is a vec of leaf hashes for the newly created UTXOs. The second one is the position
    /// for the UTXOs being spent in this block as inputs.
    ///
    /// # Example
    /// ```
    /// use bitcoin_hashes::sha256::Hash as Data;
    /// use bitcoin_hashes::Hash;
    /// use bitcoin_hashes::HashEngine;
    /// use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    /// use rustreexo::accumulator::pollard::Pollard;
    /// let values = vec![0, 1, 2, 3, 4, 5, 6, 7];
    /// let hashes = values
    ///     .into_iter()
    ///     .map(|val| {
    ///         let mut engine = Data::engine();
    ///         engine.input(&[val]);
    ///         BitcoinNodeHash::from(Data::from_engine(engine).as_byte_array())
    ///     })
    ///     .collect::<Vec<_>>();
    /// // Add 8 leaves to the pollard
    /// let mut p = Pollard::new();
    /// p.modify(&hashes, &[]).expect("Pollard should not fail");
    ///
    /// assert_eq!(
    ///     p.get_roots()[0].get_data().to_string(),
    ///     String::from("b151a956139bb821d4effa34ea95c17560e0135d1e4661fc23cedc3af49dac42")
    /// );
    /// ```
    pub fn modify(
        &mut self,
        add: &[BitcoinNodeHash],
        del: &[BitcoinNodeHash],
    ) -> Result<(), String> {
        self.del(del)?;
        self.add(add);
        Ok(())
    }
    #[allow(clippy::type_complexity)]
    pub fn grab_node(&self, pos: u64) -> Result<(Rc<Node>, Rc<Node>, Rc<Node>), String> {
        let (tree, branch_len, bits) = detect_offset(pos, self.leaves);
        let mut n = Some(self.roots[tree as usize].clone());
        let mut sibling = Some(self.roots[tree as usize].clone());
        let mut parent = sibling.clone();

        for row in (0..(branch_len)).rev() {
            // Parent is the sibling of the current node as each of the
            // nodes point to their nieces.
            parent.clone_from(&sibling);

            // Figure out which node we need to follow.
            let niece_pos = ((bits >> row) & 1) as u8;

            #[allow(clippy::assigning_clones)]
            if let Some(node) = n {
                if is_left_niece(niece_pos as u64) {
                    n = node.right.borrow().clone();
                    sibling.clone_from(&*node.left.borrow());
                } else {
                    n = node.left.borrow().clone();
                    sibling.clone_from(&*node.right.borrow());
                }
            } else {
                sibling = None;
            }
        }
        if let (Some(node), Some(sibling), Some(parent)) = (n, sibling, parent) {
            return Ok((node, sibling, parent));
        }
        Err(format!("node {} not found", pos))
    }
    fn del(&mut self, targets: &[BitcoinNodeHash]) -> Result<(), String> {
        let mut pos = targets
            .iter()
            .flat_map(|target| self.map.get(target))
            .flat_map(|target| target.upgrade())
            .map(|target| {
                (
                    self.get_pos(self.map.get(&target.data.get()).unwrap()),
                    target.data.get(),
                )
            })
            .collect::<Vec<_>>();

        pos.sort();
        let (_, targets): (Vec<u64>, Vec<BitcoinNodeHash>) = pos.into_iter().unzip();
        for target in targets {
            match self.map.remove(&target) {
                Some(target) => {
                    let mut tgt = target.upgrade().unwrap().clone().as_ref().clone();
                    self.del_single(&mut tgt);
                }
                None => {
                    return Err(format!("node {} not in the forest", target));
                }
            }
        }
        Ok(())
    }
    pub fn verify(&self, proof: &Proof, del_hashes: &[BitcoinNodeHash]) -> Result<bool, String> {
        let roots = self
            .roots
            .iter()
            .map(|root| root.get_data())
            .collect::<Vec<_>>();
        proof.verify(del_hashes, &roots, self.leaves)
    }
    /// Can return wrong pos but sets flags
    pub fn fake_get_pos(&self, node: &Weak<Node>) -> u64 {
        // This indicates whether the node is a left or right child at each level
        // When we go down the tree, we can use the indicator to know which
        // child to take.
        let mut left_child_indicator = 0_u64;
        let mut rows_to_top = 0;
        let mut node = node.upgrade().unwrap();
        node.used.set(true);
        while let Some(parent) = node.parent.clone().into_inner() {
            node.parent
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .used
                .set(true);
            let parent_left = parent
                .upgrade()
                .and_then(|parent| parent.left.clone().into_inner())
                .unwrap()
                .clone();
            parent_left.used.set(true);

            // If the current node is a left child, we left-shift the indicator
            // and leave the LSB as 0
            if parent_left.get_data() == node.get_data() {
                left_child_indicator <<= 1;
            } else {
                // If the current node is a right child, we left-shift the indicator
                // and set the LSB to 1
                left_child_indicator <<= 1;
                left_child_indicator |= 1;
            }
            rows_to_top += 1;
            node = parent.upgrade().unwrap();
        }
        let mut root_idx = self.roots.len() - 1;
        let forest_rows = tree_rows(self.leaves);
        let mut root_row = 0;
        // Find the root of the tree that the node belongs to
        for row in 0..forest_rows {
            if is_root_populated(row, self.leaves) {
                let root = &self.roots[root_idx];
                if root.get_data() == node.get_data() {
                    root_row = row;
                    break;
                }
                root_idx -= 1;
            }
        }
        let mut pos = root_position(self.leaves, root_row, forest_rows);
        for _ in 0..rows_to_top {
            // If LSB is 0, go left, otherwise go right
            match left_child_indicator & 1 {
                0 => {
                    pos = left_child(pos, forest_rows);
                }
                1 => {
                    pos = right_child(pos, forest_rows);
                }
                _ => unreachable!(),
            }
            left_child_indicator >>= 1;
        }
        pos
    }

    fn get_pos(&self, node: &Weak<Node>) -> u64 {
        // This indicates whether the node is a left or right child at each level
        // When we go down the tree, we can use the indicator to know which
        // child to take.
        let mut left_child_indicator = 0_u64;
        let mut rows_to_top = 0;
        let mut node = node.upgrade().unwrap();
        while let Some(parent) = node.parent.clone().into_inner() {
            let parent_left = parent
                .upgrade()
                .and_then(|parent| parent.left.clone().into_inner())
                .unwrap()
                .clone();

            // If the current node is a left child, we left-shift the indicator
            // and leave the LSB as 0
            if parent_left.get_data() == node.get_data() {
                left_child_indicator <<= 1;
            } else {
                // If the current node is a right child, we left-shift the indicator
                // and set the LSB to 1
                left_child_indicator <<= 1;
                left_child_indicator |= 1;
            }
            rows_to_top += 1;
            node = parent.upgrade().unwrap();
        }
        let mut root_idx = self.roots.len() - 1;
        let forest_rows = tree_rows(self.leaves);
        let mut root_row = 0;
        // Find the root of the tree that the node belongs to
        for row in 0..forest_rows {
            if is_root_populated(row, self.leaves) {
                let root = &self.roots[root_idx];
                if root.get_data() == node.get_data() {
                    root_row = row;
                    break;
                }
                root_idx -= 1;
            }
        }
        let mut pos = root_position(self.leaves, root_row, forest_rows);
        for _ in 0..rows_to_top {
            // If LSB is 0, go left, otherwise go right
            match left_child_indicator & 1 {
                0 => {
                    pos = left_child(pos, forest_rows);
                }
                1 => {
                    pos = right_child(pos, forest_rows);
                }
                _ => unreachable!(),
            }
            left_child_indicator >>= 1;
        }
        pos
    }
    fn del_single(&mut self, node: &mut Node) -> Option<()> {
        let parent = node.parent.borrow();

        // Deleting a root
        let parent = match *parent {
            Some(ref node) => node.upgrade()?,
            None => {
                let pos = self.roots.iter().position(|x| x.data == node.data).unwrap();
                self.roots[pos] = Rc::new(Node {
                    ty: NodeType::Branch,
                    parent: RefCell::new(None),
                    data: Cell::new(BitcoinNodeHash::default()),
                    left: RefCell::new(None),
                    right: RefCell::new(None),
                    used: Cell::new(true),
                });
                return None;
            }
        };

        parent.used.set(true);

        let me = parent.left.borrow();
        // Can unwrap because we know the sibling exists
        let sibling = if me.as_deref()?.data == node.data {
            parent.right.borrow().clone()
        } else {
            parent.left.borrow().clone()
        };
        if let Some(ref sibling) = sibling {
            sibling.used.set(true);

            let grandparent = parent.parent.borrow().clone();

            if let Some(ref gp) = grandparent {
                gp.upgrade().unwrap().used.set(true);
            }

            sibling.parent.replace(grandparent.clone());

            if let Some(ref grandparent) = grandparent.and_then(|g| g.upgrade()) {
                if grandparent.left.borrow().clone().as_ref().unwrap().data == parent.data {
                    grandparent.left.borrow().clone().unwrap().used.set(true);
                    grandparent.left.replace(Some(sibling.clone()));
                } else {
                    grandparent.right.borrow().clone().unwrap().used.set(true);
                    grandparent.right.replace(Some(sibling.clone()));
                }
                sibling.recompute_hashes();
            } else {
                let pos = self
                    .roots
                    .iter()
                    .position(|x| x.data == parent.data)
                    .unwrap();
                self.roots[pos] = sibling.clone();
            }
        };

        Some(())
    }
    fn add_single(&mut self, value: BitcoinNodeHash) {
        let mut node: Rc<Node> = Rc::new(Node {
            ty: NodeType::Leaf,
            parent: RefCell::new(None),
            data: Cell::new(value),
            left: RefCell::new(None),
            right: RefCell::new(None),
            used: Cell::new(true),
        });
        self.map.insert(value, Rc::downgrade(&node));
        let mut leaves = self.leaves;
        while leaves & 1 != 0 {
            let root = self.roots.pop().unwrap();
            root.used.set(true);
            if root.get_data() == BitcoinNodeHash::empty() {
                leaves >>= 1;
                continue;
            }
            let new_node = Rc::new(Node {
                ty: NodeType::Branch,
                parent: RefCell::new(None),
                data: Cell::new(BitcoinNodeHash::parent_hash(
                    &root.data.get(),
                    &node.data.get(),
                )),
                left: RefCell::new(Some(root.clone())),
                right: RefCell::new(Some(node.clone())),
                used: Cell::new(true),
            });
            root.parent.replace(Some(Rc::downgrade(&new_node)));
            root.parent
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .used
                .set(true);
            node.parent.replace(Some(Rc::downgrade(&new_node)));
            node.parent
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .used
                .set(true);

            node = new_node;
            leaves >>= 1;
        }
        self.roots.push(node);
        self.leaves += 1;
    }
    fn add(&mut self, values: &[BitcoinNodeHash]) {
        for value in values {
            self.add_single(*value);
        }
    }
    /// to_string returns the full pollard in a string for all forests less than 6 rows.
    fn string(&self) -> String {
        if self.leaves == 0 {
            return "empty".to_owned();
        }
        let fh = tree_rows(self.leaves);
        // The accumulator should be less than 6 rows.
        if fh > 6 {
            let s = format!("Can't print {} leaves. roots: \n", self.leaves);
            return self.get_roots().iter().fold(s, |mut a, b| {
                a.extend(format!("{}\n", b.get_data()).chars());
                a
            });
        }
        let mut output = vec!["".to_string(); (fh as usize * 2) + 1];
        let mut pos: u8 = 0;
        for h in 0..=fh {
            let row_len = 1 << (fh - h);
            for _ in 0..row_len {
                let max = max_position_at_row(h, fh, self.leaves).unwrap();
                if max >= pos as u64 {
                    match self.get_hash(pos as u64) {
                        Ok(val) => {
                            if pos >= 100 {
                                output[h as usize * 2].push_str(
                                    format!("{:#02x}:{} ", pos, &val.to_string()[..2]).as_str(),
                                );
                            } else {
                                output[h as usize * 2].push_str(
                                    format!("{:0>2}:{} ", pos, &val.to_string()[..4]).as_str(),
                                );
                            }
                        }
                        Err(_) => {
                            output[h as usize * 2].push_str("        ");
                        }
                    }
                }

                if h > 0 {
                    output[(h as usize * 2) - 1].push_str("|-------");

                    for _ in 0..((1 << h) - 1) / 2 {
                        output[(h as usize * 2) - 1].push_str("--------");
                    }
                    output[(h as usize * 2) - 1].push_str("\\       ");

                    for _ in 0..((1 << h) - 1) / 2 {
                        output[(h as usize * 2) - 1].push_str("        ");
                    }

                    for _ in 0..(1 << h) - 1 {
                        output[h as usize * 2].push_str("        ");
                    }
                }
                pos += 1;
            }
        }

        output.iter().rev().fold(String::new(), |mut a, b| {
            a.push_str(b);
            a.push('\n');
            a
        })
    }
}

impl Debug for Pollard {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.string())
    }
}
impl Display for Pollard {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.string())
    }
}
#[cfg(test)]
mod test {
    use std::rc::Rc;
    use std::str::FromStr;
    use std::vec;

    use bitcoin_hashes::sha256::Hash as Data;
    use bitcoin_hashes::HashEngine;
    use serde::de::DeserializeOwned;
    use serde::Deserialize;

    use super::Pollard;
    use crate::accumulator::node_hash::BitcoinNodeHash;
    use crate::accumulator::pollard::Node;
    use crate::accumulator::proof::Proof;

    fn hash_from_u8(value: u8) -> BitcoinNodeHash {
        let mut engine = Data::engine();

        engine.input(&[value]);

        BitcoinNodeHash::from(Data::from_engine(engine).as_byte_array())
    }

    #[test]
    fn test_grab_node() {
        let values = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        let hashes = values.into_iter().map(hash_from_u8).collect::<Vec<_>>();

        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Pollard should not fail");
        let (found_target, found_sibling, _) = p.grab_node(4).unwrap();
        let target = BitcoinNodeHash::from_str(
            "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
        )
        .unwrap();
        let sibling = BitcoinNodeHash::from_str(
            "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
        )
        .unwrap();

        assert_eq!(target, found_target.data.get());
        assert_eq!(sibling, found_sibling.data.get());
    }

    #[test]
    fn test_delete() {
        let values = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let hashes = values.into_iter().map(hash_from_u8).collect::<Vec<_>>();

        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Pollard should not fail");
        p.modify(&[], &[hashes[0]]).expect("msg");

        let (node, _, _) = p.grab_node(8).unwrap();
        assert_eq!(
            String::from("4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a"),
            node.data.get().to_string()
        );
    }
    #[test]
    fn test_proof_verify() {
        let values = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let hashes = values.into_iter().map(hash_from_u8).collect::<Vec<_>>();
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).unwrap();

        let proof = p.prove(&[hashes[0], hashes[1]]).unwrap();
        assert!(p.verify(&proof, &[hashes[0], hashes[1]]).unwrap());
    }
    #[test]
    fn test_add() {
        let values = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
        let hashes = values.into_iter().map(hash_from_u8).collect::<Vec<_>>();

        let mut acc = Pollard::new();
        acc.add(&hashes);

        assert_eq!(
            "b151a956139bb821d4effa34ea95c17560e0135d1e4661fc23cedc3af49dac42",
            acc.roots[0].data.get().to_string().as_str(),
        );
        assert_eq!(
            "9c053db406c1a077112189469a3aca0573d3481bef09fa3d2eda3304d7d44be8",
            acc.roots[1].data.get().to_string().as_str(),
        );
        assert_eq!(
            "55d0a0ef8f5c25a9da266b36c0c5f4b31008ece82df2512c8966bddcc27a66a0",
            acc.roots[2].data.get().to_string().as_str(),
        );
        assert_eq!(
            "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
            acc.roots[3].data.get().to_string().as_str(),
        );
    }

    #[ignore = "Too long to run each time"]
    #[test]
    fn test_pollard_size() {
        let mut p = Pollard::new();
        let mut values = vec![];
        for i in 0..1000000 {
            values.push(i as u8);
        }
        let hashes: Vec<BitcoinNodeHash> = values.into_iter().map(hash_from_u8).collect();
        p.modify(&hashes, &Vec::new()).unwrap();
        let cloned_p = p.clone();
        let _ = bincode::serialize(&cloned_p).unwrap();
        p.restore_used_flag();

        let stripped = p.get_stripped_pollard();

        let _ = bincode::serialize(&stripped).unwrap();
    }

    #[test]
    fn test_delete_non_root() {
        // Assuming this tree, if we delete `01`, 00 will move up to 08's position
        // 14
        // |-----------------\
        // 12                13
        // |-------\         |--------\
        // 08       09       10       11
        // |----\   |----\   |----\   |----\
        // 00   01  02   03  04   05  06   07

        // 14
        // |-----------------\
        // 12                13
        // |-------\         |--------\
        // 08       09       10       11
        // |----\   |----\   |----\   |----\
        // 00   01  02   03  04   05  06   07

        // Where 08's data is just 00's

        let values = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let hashes: Vec<BitcoinNodeHash> = values.into_iter().map(hash_from_u8).collect();

        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Pollard should not fail");
        p.modify(&[], &[hashes[1]]).expect("Still should not fail");

        assert_eq!(p.roots.len(), 1);
        let (node, _, _) = p.grab_node(8).expect("This tree should have pos 8");
        assert_eq!(node.data.get(), hashes[0]);
    }
    #[derive(Debug, Deserialize)]
    struct TestCase {
        leaf_preimages: Vec<u8>,
        target_values: Option<Vec<u64>>,
        expected_roots: Vec<String>,
    }
    fn run_single_addition_case(case: TestCase) {
        let hashes = case
            .leaf_preimages
            .iter()
            .map(|preimage| hash_from_u8(*preimage))
            .collect::<Vec<_>>();
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        assert_eq!(p.get_roots().len(), case.expected_roots.len());
        let expected_roots = case
            .expected_roots
            .iter()
            .map(|root| BitcoinNodeHash::from_str(root).unwrap())
            .collect::<Vec<_>>();
        let roots = p
            .get_roots()
            .iter()
            .map(|root| root.data.get())
            .collect::<Vec<_>>();
        assert_eq!(expected_roots, roots, "Test case failed {:?}", case);
    }
    fn run_case_with_deletion(case: TestCase) {
        let hashes = case
            .leaf_preimages
            .iter()
            .map(|preimage| hash_from_u8(*preimage))
            .collect::<Vec<_>>();
        let dels = case
            .target_values
            .clone()
            .unwrap()
            .iter()
            .map(|pos| hashes[*pos as usize])
            .collect::<Vec<_>>();
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        p.modify(&[], &dels).expect("still should be valid");

        assert_eq!(p.get_roots().len(), case.expected_roots.len());
        let expected_roots = case
            .expected_roots
            .iter()
            .map(|root| BitcoinNodeHash::from_str(root).unwrap())
            .collect::<Vec<_>>();
        let roots = p
            .get_roots()
            .iter()
            .map(|root| root.data.get())
            .collect::<Vec<_>>();
        assert_eq!(expected_roots, roots, "Test case failed {:?}", case);
    }
    #[test]
    fn run_tests_from_cases() {
        #[derive(Deserialize)]
        struct TestsJSON {
            insertion_tests: Vec<TestCase>,
            deletion_tests: Vec<TestCase>,
        }

        let contents = std::fs::read_to_string("test_values/test_cases.json")
            .expect("Something went wrong reading the file");

        let tests = serde_json::from_str::<TestsJSON>(contents.as_str())
            .expect("JSON deserialization error");

        for i in tests.insertion_tests {
            run_single_addition_case(i);
        }
        for i in tests.deletion_tests {
            run_case_with_deletion(i);
        }
    }
    #[test]
    fn test_to_string() {
        let hashes = get_hash_vec_of(&(0..255).collect::<Vec<_>>());
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        assert_eq!(
            Some("Can't print 255 leaves. roots:"),
            p.to_string().get(0..30)
        );
    }
    #[test]
    fn test_get_pos() {
        macro_rules! test_get_pos {
            ($p:ident, $pos:literal) => {
                assert_eq!(
                    $p.get_pos(&Rc::downgrade(&$p.grab_node($pos).unwrap().0)),
                    $pos
                );
            };
        }
        let hashes = get_hash_vec_of(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        test_get_pos!(p, 0);
        test_get_pos!(p, 1);
        test_get_pos!(p, 2);
        test_get_pos!(p, 3);
        test_get_pos!(p, 4);
        test_get_pos!(p, 5);
        test_get_pos!(p, 6);
        test_get_pos!(p, 7);
        test_get_pos!(p, 8);
        test_get_pos!(p, 9);
        test_get_pos!(p, 10);
        test_get_pos!(p, 11);
        test_get_pos!(p, 12);

        assert_eq!(p.get_pos(&Rc::downgrade(&p.get_roots()[0])), 28);
        assert_eq!(
            p.get_pos(&Rc::downgrade(
                p.get_roots()[0].left.borrow().as_ref().unwrap()
            )),
            24
        );
        assert_eq!(
            p.get_pos(&Rc::downgrade(
                p.get_roots()[0].right.borrow().as_ref().unwrap()
            )),
            25
        );
    }
    #[test]
    fn test_serialize_one() {
        let hashes = get_hash_vec_of(&[0, 1, 2, 3, 4, 5, 6, 7]);
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        p.modify(&[], &[hashes[0]]).expect("can remove 0");
        let mut writer = std::io::Cursor::new(Vec::new());
        p.get_roots()[0].write_one(&mut writer).unwrap();
        let (deserialized, _) =
            Node::read_one(&mut std::io::Cursor::new(writer.into_inner())).unwrap();
        assert_eq!(deserialized.get_data(), p.get_roots()[0].get_data());
    }
    #[test]
    fn test_serialization() {
        let hashes = get_hash_vec_of(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");
        p.modify(&[], &[hashes[0]]).expect("can remove 0");
        let mut writer = std::io::Cursor::new(Vec::new());
        p.serialize(&mut writer).unwrap();
        let deserialized =
            Pollard::deserialize(&mut std::io::Cursor::new(writer.into_inner())).unwrap();
        assert_eq!(
            deserialized.get_roots()[0].get_data(),
            p.get_roots()[0].get_data()
        );
        assert_eq!(deserialized.leaves, p.leaves);
        assert_eq!(deserialized.map.len(), p.map.len());
    }
    #[test]
    fn test_proof() {
        let hashes = get_hash_vec_of(&[0, 1, 2, 3, 4, 5, 6, 7]);
        let del_hashes = [hashes[2], hashes[1], hashes[4], hashes[6]];

        let mut p = Pollard::new();
        p.modify(&hashes, &[]).expect("Test pollards are valid");

        let proof = p.prove(&del_hashes).expect("Should be able to prove");

        let expected_proof = Proof::new(
            [2, 1, 4, 6].to_vec(),
            vec![
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
                    .parse()
                    .unwrap(),
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5"
                    .parse()
                    .unwrap(),
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db"
                    .parse()
                    .unwrap(),
                "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879"
                    .parse()
                    .unwrap(),
            ],
        );
        assert_eq!(proof, expected_proof);
        assert!(p.verify(&proof, &del_hashes).unwrap());
    }
    fn get_hash_vec_of(elements: &[u8]) -> Vec<BitcoinNodeHash> {
        elements.iter().map(|el| hash_from_u8(*el)).collect()
    }

    #[test]
    fn test_display_empty() {
        let p = Pollard::new();
        let _ = p.to_string();
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut p = Pollard::new();
        let values = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let hashes: Vec<BitcoinNodeHash> = values
            .into_iter()
            .map(|i| BitcoinNodeHash::from([i; 32]))
            .collect();
        p.modify(&hashes, &[]).expect("modify should work");
        assert_eq!(p.get_roots().len(), 1);
        assert!(!p.get_roots()[0].get_data().is_empty());
        assert_eq!(p.leaves, 16);
        p.modify(&[], &hashes).expect("modify should work");
        assert_eq!(p.get_roots().len(), 1);
        assert!(p.get_roots()[0].get_data().is_empty());
        assert_eq!(p.leaves, 16);
        let mut serialized = Vec::<u8>::new();
        p.serialize(&mut serialized).expect("serialize should work");
        let deserialized = Pollard::deserialize(&*serialized).expect("deserialize should work");
        assert_eq!(deserialized.get_roots().len(), 1);
        assert!(deserialized.get_roots()[0].get_data().is_empty());
        assert_eq!(deserialized.leaves, 16);
    }

    #[test]
    fn test_serialization_roundtrip2() {
        let mut p = Pollard::new();
        let values = vec![1, 2, 3, 4, 5];
        let hashes: Vec<BitcoinNodeHash> = values
            .into_iter()
            .map(|i| BitcoinNodeHash::from([i; 32]))
            .collect();

        let _ = p.modify(&hashes, &[]);

        println!("{:#?}", p);

        p.restore_used_flag();

        let markuped = p.fake_modify(&[BitcoinNodeHash::from(&[1_u8; 32])], &[]);

        for root in markuped.get_roots() {
            println!("Root used? - {:#?}", root.used.get());
        }

        let stripped = markuped.get_stripped_pollard();

        println!("\n\n{:#?}", stripped);

        // let mut serialized = Vec::<u8>::new();
        // p.serialize(&mut serialized).expect("serialize should work");
        // let deserialized = Pollard::deserialize(&*serialized).expect("deserialize should work");
    }

    #[test]
    fn test_is_pollard_deserialize_owned() {
        is_pollard_deserialize_owned();
    }

    #[test]
    fn test_bincode_serialization() {
        let hashes = get_hash_vec_of(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        let mut pollard = Pollard::new();
        pollard
            .modify(&hashes, &[])
            .expect("Test pollards are valid");
        // Serialize with bincode
        let binary_data = bincode::serialize(&pollard).expect("Serialization failed");

        // Deserialize with bincode
        let deserialized_pollard: Pollard =
            bincode::deserialize(&binary_data).expect("Deserialization failed");

        // Assert equality
        let old_roots = pollard.get_roots();
        let new_roots = deserialized_pollard.get_roots();

        assert_eq!(old_roots.len(), new_roots.len());
        for (old_root, new_root) in old_roots.iter().zip(new_roots.iter()) {
            assert_eq!(old_root.get_data(), new_root.get_data());
        }
    }

    fn is_pollard_deserialize_owned() {
        let acc = Pollard::new();
        let _ = deserialize_owned_need_foo(acc);
    }

    fn deserialize_owned_need_foo<T>(acc: T) -> Result<T, u32>
    where
        T: DeserializeOwned,
    {
        Ok(acc)
    }
}
