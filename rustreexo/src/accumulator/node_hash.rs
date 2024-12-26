//! [NodeHash] is an internal type for representing Hashes in an utreexo accumulator. It's
//! just a wrapper around [[u8; 32]] but with some useful methods.
//! # Examples
//! Building from a str
//! ```
//! use std::str::FromStr;
//!
//! use rustreexo::accumulator::node_hash::NodeHash;
//! let hash =
//!     NodeHash::from_str("0000000000000000000000000000000000000000000000000000000000000000")
//!         .unwrap();
//! assert_eq!(
//!     hash.to_string().as_str(),
//!     "0000000000000000000000000000000000000000000000000000000000000000"
//! );
//! ```
//! Building from a slice
//! ```
//! use std::str::FromStr;
//!
//! use rustreexo::accumulator::node_hash::NodeHash;
//! let hash1 = NodeHash::new([0; 32]);
//! // ... or ...
//! let hash2 = NodeHash::from([0; 32]);
//! assert_eq!(hash1, hash2);
//! assert_eq!(
//!     hash1.to_string().as_str(),
//!     "0000000000000000000000000000000000000000000000000000000000000000"
//! );
//! ```
//!
//! Computing a parent hash (i.e a hash of two nodes concatenated)
//! ```
//! use std::str::FromStr;
//!
//! use rustreexo::accumulator::node_hash::NodeHash;
//! let left = NodeHash::new([0; 32]);
//! let right = NodeHash::new([1; 32]);
//! let parent = NodeHash::parent_hash(&left, &right);
//! let expected_parent =
//!     NodeHash::from_str("34e33ca0c40b7bd33d28932ca9e35170def7309a3bf91ecda5e1ceb067548a12")
//!         .unwrap();
//! assert_eq!(parent, expected_parent);
//! ```
use std::convert::TryFrom;
use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;

use bitcoin_hashes::hex;
use bitcoin_hashes::sha256;
use bitcoin_hashes::sha512_256;
use bitcoin_hashes::Hash;
#[cfg(feature = "with-serde")]
use serde::Deserialize;
#[cfg(feature = "with-serde")]
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

#[derive(Eq, PartialEq, Copy, Clone, Hash, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
/// NodeHash is a wrapper around a 32 byte array that represents a hash of a node in the tree.
/// # Example
/// ```
/// use rustreexo::accumulator::node_hash::NodeHash;
/// let hash = NodeHash::new([0; 32]);
/// assert_eq!(
///     hash.to_string().as_str(),
///     "0000000000000000000000000000000000000000000000000000000000000000"
/// );
/// ```
pub enum BitcoinNodeHash {
    #[default]
    Empty,
    Placeholder,
    Some([u8; 32]),
}

impl Deref for BitcoinNodeHash {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        match self {
            BitcoinNodeHash::Some(ref inner) => inner,
            _ => &[0; 32],
        }
    }
}
impl Display for BitcoinNodeHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        if let BitcoinNodeHash::Some(ref inner) = self {
            let mut s = String::new();
            for byte in inner.iter() {
                s.push_str(&format!("{:02x}", byte));
            }
            write!(f, "{}", s)
        } else {
            write!(f, "empty")
        }
    }
}
impl Debug for BitcoinNodeHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        if let BitcoinNodeHash::Some(ref inner) = self {
            let mut s = String::new();
            for byte in inner.iter() {
                s.push_str(&format!("{:02x}", byte));
            }
            write!(f, "{}", s)
        } else {
            write!(f, "empty")
        }
    }
}
impl From<sha512_256::Hash> for BitcoinNodeHash {
    fn from(hash: sha512_256::Hash) -> Self {
        BitcoinNodeHash::Some(hash.to_byte_array())
    }
}
impl From<[u8; 32]> for BitcoinNodeHash {
    fn from(hash: [u8; 32]) -> Self {
        BitcoinNodeHash::Some(hash)
    }
}
impl From<&[u8; 32]> for BitcoinNodeHash {
    fn from(hash: &[u8; 32]) -> Self {
        BitcoinNodeHash::Some(*hash)
    }
}
#[cfg(test)]
impl TryFrom<&str> for BitcoinNodeHash {
    type Error = hex::HexToArrayError;
    fn try_from(hash: &str) -> Result<Self, Self::Error> {
        // This implementation is useful for testing, as it allows to create empty hashes
        // from the string of 64 zeros. Without this, it would be impossible to express this
        // hash in the test vectors.
        if hash == "0000000000000000000000000000000000000000000000000000000000000000" {
            return Ok(BitcoinNodeHash::Empty);
        }
        let hash = hex::FromHex::from_hex(hash)?;
        Ok(BitcoinNodeHash::Some(hash))
    }
}

#[cfg(not(test))]
impl TryFrom<&str> for BitcoinNodeHash {
    type Error = hex::HexToArrayError;
    fn try_from(hash: &str) -> Result<Self, Self::Error> {
        let inner = hex::FromHex::from_hex(hash)?;
        Ok(BitcoinNodeHash::Some(inner))
    }
}
impl From<&[u8]> for BitcoinNodeHash {
    fn from(hash: &[u8]) -> Self {
        let mut inner = [0; 32];
        inner.copy_from_slice(hash);
        BitcoinNodeHash::Some(inner)
    }
}

impl From<sha256::Hash> for BitcoinNodeHash {
    fn from(hash: sha256::Hash) -> Self {
        BitcoinNodeHash::Some(hash.to_byte_array())
    }
}
impl FromStr for BitcoinNodeHash {
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BitcoinNodeHash::try_from(s)
    }
    type Err = hex::HexToArrayError;
}
impl BitcoinNodeHash {
    /// Tells whether this hash is empty. We use empty hashes throughout the code to represent
    /// leaves we want to delete.
    pub fn is_empty(&self) -> bool {
        if let BitcoinNodeHash::Empty = self {
            return true;
        }
        false
    }
    /// Creates a new BitcoinNodeHash from a 32 byte array.
    /// # Example
    /// ```
    /// use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    /// let hash = BitcoinNodeHash::new([0; 32]);
    /// assert_eq!(
    ///     hash.to_string().as_str(),
    ///     "0000000000000000000000000000000000000000000000000000000000000000"
    /// );
    /// ```
    pub fn new(inner: [u8; 32]) -> Self {
        BitcoinNodeHash::Some(inner)
    }
    /// Creates an empty hash. This is used to represent leaves we want to delete.
    /// # Example
    /// ```
    /// use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    /// let hash = BitcoinNodeHash::empty();
    /// assert!(hash.is_empty());
    /// ```
    pub fn empty() -> Self {
        BitcoinNodeHash::Empty
    }
    /// parent_hash return the merkle parent of the two passed in nodes.
    /// # Example
    /// ```
    /// use std::str::FromStr;
    ///
    /// use rustreexo::accumulator::node_hash::BitcoinNodeHash;
    /// let left = BitcoinNodeHash::new([0; 32]);
    /// let right = BitcoinNodeHash::new([1; 32]);
    /// let parent = BitcoinNodeHash::parent_hash(&left, &right);
    /// let expected_parent = BitcoinNodeHash::from_str(
    ///     "34e33ca0c40b7bd33d28932ca9e35170def7309a3bf91ecda5e1ceb067548a12",
    /// )
    /// .unwrap();
    /// assert_eq!(parent, expected_parent);
    /// ```
    pub fn parent_hash(left: &BitcoinNodeHash, right: &BitcoinNodeHash) -> BitcoinNodeHash {
        let mut hasher = Sha256::new();
        hasher.update(left.as_slice());
        hasher.update(right.as_slice());
        let result = hasher.finalize();
        BitcoinNodeHash::from(result.as_slice())

        // println!("parent hash called");
        // let mut hash = sha512_256::Hash::engine();
        // hash.input(&**left);
        // hash.input(&**right);
        // sha512_256::Hash::from_engine(hash).into()
    }

    /// Returns a arbitrary placeholder hash that is unlikely to collide with any other hash.
    /// We use this while computing roots to destroy. Don't confuse this with an empty hash.
    pub const fn placeholder() -> Self {
        BitcoinNodeHash::Placeholder
    }

    /// write to buffer
    pub(super) fn write<W>(&self, writer: &mut W) -> std::io::Result<()>
    where
        W: std::io::Write,
    {
        match self {
            Self::Empty => writer.write_all(&[0]),
            Self::Placeholder => writer.write_all(&[1]),
            Self::Some(hash) => {
                writer.write_all(&[2])?;
                writer.write_all(hash)
            }
        }
    }

    /// Read from buffer
    pub(super) fn read<R>(reader: &mut R) -> std::io::Result<Self>
    where
        R: std::io::Read,
    {
        let mut tag = [0];
        reader.read_exact(&mut tag)?;
        match tag {
            [0] => Ok(Self::Empty),
            [1] => Ok(Self::Placeholder),
            [2] => {
                let mut hash = [0; 32];
                reader.read_exact(&mut hash)?;
                Ok(Self::Some(hash))
            }
            [_] => {
                let err = std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unexpected tag for BitcoinNodeHash",
                );
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::BitcoinNodeHash;
    use crate::accumulator::util::hash_from_u8;

    #[test]
    fn test_parent_hash() {
        let hash1 = hash_from_u8(0);
        let hash2 = hash_from_u8(1);

        let parent_hash = BitcoinNodeHash::parent_hash(&hash1, &hash2);
        assert_eq!(
            parent_hash.to_string().as_str(),
            "02242b37d8e851f1e86f46790298c7097df06893d6226b7c1453c213e91717de"
        );
    }
    #[test]
    fn test_hash_from_str() {
        let hash = BitcoinNodeHash::from_str(
            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
        )
        .unwrap();
        assert_eq!(hash, hash_from_u8(0));
    }
    #[test]
    fn test_empty_hash() {
        // Only relevant for tests
        let hash = BitcoinNodeHash::from_str(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        assert_eq!(hash, BitcoinNodeHash::empty());
    }
}
