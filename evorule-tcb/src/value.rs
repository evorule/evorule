// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! JSON value types - deterministic data model
//!
//! # Design principles
//! - Zero-clone first (uses `Cow` to avoid unnecessary copies)
//! - Deterministic iteration (`BTreeMap`)
//! - Type safety (all conversions return `Option`)
//! - Never panics

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

#[cfg(not(kani))]
use alloc::collections::BTreeMap;

/// Kani-only Object backend: an ordered `Vec` emulating `BTreeMap` (enabled under `cfg(kani)`)
///
/// # Root cause (measured 2026-09-13 via probes; located from 23 tier-B proof timeouts)
///
/// The CBMC backend of Kani 0.67 models `BTreeMap` as a red-black tree: inserting
/// ≥2 keys causes state explosion (90s+ without convergence; a larger unwind does
/// not help); the ordered `Vec` needs only 2-3s for a 3-key insert + get. A `Vec`
/// has no pointer tracking and no recursive tree traversal, so CBMC's linear
/// unrolling covers it.
///
/// # Semantic equivalence guarantees
///
/// - Entries are always ordered by key lexicographically (`insert` locates the slot
///   linearly and maintains the invariant by positional insertion); `iter()` yields
///   the same order as `BTreeMap`;
/// - API signatures are compatible with the `BTreeMap<String, JsonValue>` methods
///   the TCB/reactor uses (`get`/`get_mut`/`insert`/`remove`/`contains_key`/
///   `values`/`iter`, etc.);
/// - Production builds (`cfg(not(kani))`) are unaffected; `ObjectMap` stays `BTreeMap`.
#[cfg(kani)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KaniMap {
    /// Ordered entries (key lexicographic order, maintained by `insert`)
    entries: Vec<(String, JsonValue)>,
}

/// Kani-only no-op Drop (same pattern as the JsonValue no-op Drop; root cause in its
/// doc comment): prevents the compiler-synthesized recursive destruction of the
/// entries field (Vec → per-element slice → String RawVec::deallocate), which CBMC
/// unrolls iteration by iteration (measured: verify_evaluate_domain_eq 11GB+ OOM;
/// unwind logs show the Vec<(String, JsonValue)>/KaniMap drop chain iterating 10-13
/// levels).
#[cfg(kani)]
impl Drop for KaniMap {
    fn drop(&mut self) {
        // Same true-no-op pattern as JsonValue Drop: take leaves an empty Vec,
        // forget skips destructing the taken entries; synthesized field drop then
        // destructs an empty Vec (nothing).
        let leaked = core::mem::take(&mut self.entries);
        core::mem::forget(leaked);
    }
}

#[cfg(kani)]
impl KaniMap {
    /// Key location: returns `Ok(index)` on hit; `Err(insertion point)` on miss (the first position whose key is greater)
    fn locate(&self, key: &str) -> Result<usize, usize> {
        for (i, (k, _)) in self.entries.iter().enumerate() {
            if k.as_str() == key {
                return Ok(i);
            }
            if k.as_str() > key {
                return Err(i);
            }
        }
        Err(self.entries.len())
    }

    /// Constructs an empty map
    pub fn new() -> Self {
        KaniMap {
            entries: Vec::new(),
        }
    }

    /// Inserts or updates, returning the old value (semantics match `BTreeMap::insert`)
    pub fn insert(&mut self, key: String, value: JsonValue) -> Option<JsonValue> {
        match self.locate(&key) {
            Ok(i) => self
                .entries
                .get_mut(i)
                .map(|slot| core::mem::replace(&mut slot.1, value)),
            Err(pos) => {
                self.entries.insert(pos, (key, value));
                None
            }
        }
    }

    /// Lookup (semantics match `BTreeMap::get`)
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self.locate(key) {
            Ok(i) => self.entries.get(i).map(|(_, v)| v),
            Err(_) => None,
        }
    }

    /// Mutable lookup (semantics match `BTreeMap::get_mut`)
    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        match self.locate(key) {
            Ok(i) => match self.entries.get_mut(i) {
                Some((_, v)) => Some(v),
                None => None,
            },
            Err(_) => None,
        }
    }

    /// Removes and returns the old value (semantics match `BTreeMap::remove`)
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self.locate(key) {
            Ok(i) => {
                let (_, value) = self.entries.remove(i);
                Some(value)
            }
            Err(_) => None,
        }
    }

    /// Key existence check (semantics match `BTreeMap::contains_key`)
    pub fn contains_key(&self, key: &str) -> bool {
        self.locate(key).is_ok()
    }

    /// Value iterator (semantics match `BTreeMap::values`)
    pub fn values(&self) -> impl Iterator<Item = &JsonValue> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// Ordered entry iterator (iteration order matches `BTreeMap::iter`; keys lexicographic)
    pub fn iter(&self) -> core::slice::Iter<'_, (String, JsonValue)> {
        self.entries.iter()
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(kani)]
impl Default for KaniMap {
    fn default() -> Self {
        KaniMap::new()
    }
}

#[cfg(kani)]
impl<'a> IntoIterator for &'a KaniMap {
    type Item = &'a (String, JsonValue);
    type IntoIter = core::slice::Iter<'a, (String, JsonValue)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(kani)]
impl IntoIterator for KaniMap {
    type Item = (String, JsonValue);
    type IntoIter = alloc::vec::IntoIter<(String, JsonValue)>;

    fn into_iter(mut self) -> Self::IntoIter {
        // mem::take: whole-value move (partial move-out rejected for Drop types)
        core::mem::take(&mut self.entries).into_iter()
    }
}

#[cfg(kani)]
impl<const N: usize> From<[(String, JsonValue); N]> for KaniMap {
    fn from(arr: [(String, JsonValue); N]) -> Self {
        let mut map = KaniMap::new();
        for (k, v) in arr {
            map.insert(k, v);
        }
        map
    }
}

/// Object backend type (BTreeMap guarantees deterministic iteration)
///
/// Under Kani builds (`cfg(kani)`) this switches to [`KaniMap`]: CBMC state-explodes
/// on `BTreeMap` red-black tree insertion of ≥2 keys (root cause of the 23 tier-B
/// proof timeouts, see the `KaniMap` docs); the ordered `Vec` backend keeps API
/// compatibility, an identical iteration order, and zero production impact.
#[cfg(not(kani))]
pub type ObjectMap = BTreeMap<String, JsonValue>;
/// `ObjectMap` under Kani builds (see the docs above).
#[cfg(kani)]
pub type ObjectMap = KaniMap;

/// JSON value type (deterministic implementation)
///
/// # Design decisions
/// - No `Float` type (an obstacle to formal verification)
/// - `BTreeMap` guarantees deterministic iteration order
/// - `Vec` instead of `im::Vector`
/// - `Integer` is `i64` (Kani-friendly)
/// - `U64` is the full-range u64 channel (v0.9.0): `Integer` cannot losslessly carry
///   high u64 values (e.g. the `FactId(u64::MAX-1)` sentinel of audit-chain rule-set
///   anchored facts); any source whose value range may exceed `i64` must go through
///   `U64`, and `as i64` cast wraparound is banned
/// - `Cow` reduces cloning overhead
///
/// # `PartialEq` is not auto-derived
/// Manually implemented for cross-language consistency
///
/// # Examples
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::JsonValue;
/// use alloc::collections::BTreeMap;
///
/// // Construct via constructors
/// let name = JsonValue::string("Alice");
/// let age = JsonValue::Integer(30);
/// assert!(name.is_string());
/// assert_eq!(age.as_i64(), Some(30));
///
/// // Construct via From impls
/// let from_str: JsonValue = "hello".into();
/// let from_int: JsonValue = 42i64.into();
/// assert_eq!(from_str.as_str(), Some("hello"));
/// assert_eq!(from_int.as_i64(), Some(42));
///
/// // Construct a nested object
/// let mut map = BTreeMap::new();
/// map.insert("items".to_string(), JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]));
/// let v = JsonValue::object(map);
/// assert!(v.is_object());
/// ```
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON null
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer (i64)
    Integer(i64),
    /// Unsigned integer (full-range u64; strictly distinct form from [`JsonValue::Integer`],
    /// no cross-form numeric equality — see the [`PartialEq`] / [`Ord`] impls)
    U64(u64),
    /// String (uses `Cow` to reduce cloning)
    String(Cow<'static, str>),
    /// Array (ordered list)
    Array(Vec<JsonValue>),
    /// Object (key-value pairs, `BTreeMap` guarantees deterministic order)
    Object(ObjectMap),
}

/// Kani-only no-op Drop: skips modeling JsonValue's recursive destruction
///
/// Root cause (measured 2026-09-13 via probes): JsonValue's default Drop recursively
/// frees Vec/ObjectMap; CBMC unrolls every destruction level along all paths, so
/// even with fully concrete inputs the destruction of intermediate temporaries
/// (to_string products, object_from_pairs inter-layer values, evaluate_domain
/// internal temporaries) still state-explodes (verify_evaluate_domain_eq 11GB+
/// without convergence). Kani builds have no real need to free (memory is reclaimed
/// when the verification process exits); the no-op Drop makes CBMC skip destruction
/// unrolling. Production builds (cfg(not(kani))) keep the default Drop, zero impact.
#[cfg(kani)]
impl Drop for JsonValue {
    fn drop(&mut self) {
        // Rust Drop semantics: drop_in_place = user Drop::drop THEN compiler-
        // synthesized field drops. An empty body does NOT suppress recursive field
        // destruction (measured: 11GB+ CBMC OOM, drop-chain iterations 10-13). True
        // no-op: swap self to fieldless Null and forget the old value, so the
        // subsequent synthesized field drops act on Null (no recursion).
        let leaked = core::mem::replace(self, JsonValue::Null);
        core::mem::forget(leaked);
    }
}

/// Kani build: static Null reference (explicit `impl Drop` blocks rvalue
/// promotion of `&JsonValue::Null`, see E0515); non-kani keeps promotion path.
#[cfg(kani)]
static NULL_VALUE: JsonValue = JsonValue::Null;

#[cfg(not(kani))]
fn null_ref() -> &'static JsonValue {
    &JsonValue::Null
}

#[cfg(kani)]
fn null_ref() -> &'static JsonValue {
    &NULL_VALUE
}

// Manual PartialEq impl for cross-language consistency
impl PartialEq for JsonValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsonValue::Null, JsonValue::Null) => true,
            (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
            (JsonValue::Integer(a), JsonValue::Integer(b)) => a == b,
            (JsonValue::U64(a), JsonValue::U64(b)) => a == b,
            // Cross-form (Integer vs U64) is intentionally unequal: form strictness
            // matches the Ord type grouping, avoiding a fork between equality and
            // ordering semantics (callers convert forms explicitly when numeric
            // comparison is needed).
            (JsonValue::String(a), JsonValue::String(b)) => a == b,
            (JsonValue::Array(a), JsonValue::Array(b)) => a == b,
            (JsonValue::Object(a), JsonValue::Object(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                for (k, v) in a.iter() {
                    if let Some(bv) = b.get(k) {
                        if v != bv {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }
}

impl Eq for JsonValue {}

// Manual Ord impl to support ordering inside BTreeMap
impl Ord for JsonValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // Type group order: Null < Bool < Integer < U64 < String < Array < Object
            (JsonValue::Null, JsonValue::Null) => Ordering::Equal,
            (JsonValue::Null, _) => Ordering::Less,
            (_, JsonValue::Null) => Ordering::Greater,

            (JsonValue::Bool(a), JsonValue::Bool(b)) => a.cmp(b),
            (JsonValue::Bool(_), _) => Ordering::Less,
            (_, JsonValue::Bool(_)) => Ordering::Greater,

            (JsonValue::Integer(a), JsonValue::Integer(b)) => a.cmp(b),
            (JsonValue::Integer(_), _) => Ordering::Less,
            (_, JsonValue::Integer(_)) => Ordering::Greater,

            (JsonValue::U64(a), JsonValue::U64(b)) => a.cmp(b),
            (JsonValue::U64(_), _) => Ordering::Less,
            (_, JsonValue::U64(_)) => Ordering::Greater,

            (JsonValue::String(a), JsonValue::String(b)) => a.cmp(b),
            (JsonValue::String(_), _) => Ordering::Less,
            (_, JsonValue::String(_)) => Ordering::Greater,

            (JsonValue::Array(a), JsonValue::Array(b)) => a.cmp(b),
            (JsonValue::Array(_), _) => Ordering::Less,
            (_, JsonValue::Array(_)) => Ordering::Greater,

            (JsonValue::Object(a), JsonValue::Object(b)) => {
                // Compare key-value pairs lexicographically
                let mut a_iter = a.iter();
                let mut b_iter = b.iter();
                loop {
                    match (a_iter.next(), b_iter.next()) {
                        (None, None) => return Ordering::Equal,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (Some((ak, av)), Some((bk, bv))) => match ak.cmp(bk) {
                            Ordering::Equal => match av.cmp(bv) {
                                Ordering::Equal => {}
                                other => return other,
                            },
                            other => return other,
                        },
                    }
                }
            }
        }
    }
}

impl PartialOrd for JsonValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl JsonValue {
    // ===== Type checks =====

    /// Checks whether the value is an object
    pub fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    /// Checks whether the value is an array
    pub fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }

    /// Checks whether the value is an integer
    pub fn is_integer(&self) -> bool {
        matches!(self, JsonValue::Integer(_))
    }

    /// Checks whether the value is an unsigned integer (full-range u64 channel)
    pub fn is_u64(&self) -> bool {
        matches!(self, JsonValue::U64(_))
    }

    /// Checks whether the value is a string
    pub fn is_string(&self) -> bool {
        matches!(self, JsonValue::String(_))
    }

    /// Checks whether the value is a boolean
    pub fn is_bool(&self) -> bool {
        matches!(self, JsonValue::Bool(_))
    }

    /// Checks whether the value is null
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// Returns the value's length (array element count or object pair count)
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// assert_eq!(arr.len(), Some(2));
    ///
    /// let obj = JsonValue::empty_object();
    /// assert_eq!(obj.len(), Some(0));
    ///
    /// let null = JsonValue::Null;
    /// assert_eq!(null.len(), None);
    /// ```
    pub fn len(&self) -> Option<usize> {
        match self {
            JsonValue::Array(arr) => Some(arr.len()),
            JsonValue::Object(map) => Some(map.len()),
            _ => None,
        }
    }

    /// Checks whether the value is empty (array or object)
    pub fn is_empty(&self) -> bool {
        match self {
            JsonValue::Array(arr) => arr.is_empty(),
            JsonValue::Object(map) => map.is_empty(),
            _ => false,
        }
    }

    // ===== Type conversions =====

    /// Attempts conversion to i64
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Attempts conversion to u64 (only [`JsonValue::U64`] matches; strictly form-symmetric with `as_i64`)
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::U64(v) => Some(*v),
            _ => None,
        }
    }

    /// Attempts conversion to &str
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// Attempts conversion to bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Attempts conversion to &[JsonValue]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// Attempts conversion to &mut [JsonValue]
    pub fn as_array_mut(&mut self) -> Option<&mut [JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr.as_mut_slice()),
            _ => None,
        }
    }

    /// Attempts conversion to &ObjectMap
    pub fn as_object(&self) -> Option<&ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Attempts conversion to &mut ObjectMap
    pub fn as_object_mut(&mut self) -> Option<&mut ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    // ===== Object operations =====

    /// Gets an object field (if present)
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate alloc;
    /// use evorule_tcb::JsonValue;
    /// use alloc::collections::BTreeMap;
    ///
    /// let mut map = BTreeMap::new();
    /// map.insert("a".to_string(), JsonValue::Integer(1));
    /// let obj = JsonValue::object(map);
    ///
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// assert_eq!(obj.get("missing"), None);
    ///
    /// // Calling on a non-object always returns None (never panics)
    /// let n = JsonValue::Null;
    /// assert_eq!(n.get("anything"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Gets an object field (mutable)
    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        match self {
            JsonValue::Object(map) => map.get_mut(key),
            _ => None,
        }
    }

    /// Inserts or updates an object field
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut obj = JsonValue::empty_object();
    /// let old = obj.insert("k".to_string(), JsonValue::Integer(1));
    /// assert_eq!(old, None);
    ///
    /// let old = obj.insert("k".to_string(), JsonValue::Integer(2));
    /// assert_eq!(old.and_then(|v| v.as_i64()), Some(1));
    /// assert_eq!(obj.get("k").and_then(|v| v.as_i64()), Some(2));
    /// ```
    pub fn insert(&mut self, key: String, value: JsonValue) -> Option<JsonValue> {
        match self {
            JsonValue::Object(map) => map.insert(key, value),
            _ => None,
        }
    }

    /// Attempts insertion (returns an error if the key already exists)
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut obj = JsonValue::empty_object();
    /// assert!(obj.try_insert("k".to_string(), JsonValue::Integer(1)).is_ok());
    /// assert!(obj.try_insert("k".to_string(), JsonValue::Integer(2)).is_err());
    /// ```
    /// # Why an explicit `allow(clippy::map_entry)` instead of the Entry API
    ///
    /// Clippy's `map_entry` suggests rewriting with `BTreeMap::entry`, but **this
    /// function's map type is cfg-dependent** (see [`ObjectMap`]):
    ///
    /// - `cfg(not(kani))` → `BTreeMap`, which **has** the `Entry` API;
    /// - `cfg(kani)` → [`KaniMap`] (ordered `Vec`), which **has no** `Entry` API —
    ///   its very purpose is to sidestep CBMC state explosion on red-black tree
    ///   insertion.
    ///
    /// Both branches must compile the same source, so **no** backend-specific API
    /// can be used. Keeping `contains_key` + `get`/`insert` here is the **only
    /// backend-agnostic** formulation, with zero performance cost (`contains_key`
    /// and `get` traverse the same `BTreeMap` lookup path; only the return type
    /// differs). **Do not "fix" this.**
    #[allow(clippy::map_entry)]
    pub fn try_insert(&mut self, key: String, value: JsonValue) -> Result<(), &JsonValue> {
        // Backend-agnostic implementation (works for both BTreeMap / KaniMap): on
        // hit, returns a reference to the existing value; on miss, inserts. No
        // BTreeMap-specific Entry API, so the same source compiles directly after
        // ObjectMap switches to KaniMap under cfg(kani).
        // contains_key decides first (returns bool, holds no borrow); the get
        // borrow and the insert mutable borrow are separated to satisfy NLL.
        match self {
            JsonValue::Object(map) => {
                if map.contains_key(&key) {
                    match map.get(&key) {
                        Some(existing) => Err(existing),
                        None => Err(null_ref()),
                    }
                } else {
                    map.insert(key, value);
                    Ok(())
                }
            }
            _ => Err(null_ref()),
        }
    }

    /// Removes a field from the object
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self {
            JsonValue::Object(map) => map.remove(key),
            _ => None,
        }
    }

    /// Clears all content of an object or array
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// arr.clear();
    /// assert_eq!(arr.len(), Some(0));
    ///
    /// // Calling on a non-container type is a no-op
    /// let mut null = JsonValue::Null;
    /// null.clear();
    /// ```
    pub fn clear(&mut self) {
        match self {
            JsonValue::Array(arr) => arr.clear(),
            JsonValue::Object(map) => map.clear(),
            _ => {}
        }
    }
}

// ===== Display impl =====

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{b}"),
            JsonValue::Integer(i) => write!(f, "{i}"),
            JsonValue::U64(v) => write!(f, "{v}"),
            JsonValue::String(s) => escape_json_string(s.as_ref(), f),
            JsonValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            JsonValue::Object(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    escape_json_string(k, f)?;
                    write!(f, ": {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// JSON string escaping helper
///
/// Writes the string to the Formatter with JSON-standard escaping.
/// Used for both string values and object keys.
fn escape_json_string(s: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "\"")?;
    for c in s.chars() {
        match c {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\r' => write!(f, "\\r")?,
            '\t' => write!(f, "\\t")?,
            '\x08' => write!(f, "\\b")?,
            '\x0c' => write!(f, "\\f")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    write!(f, "\"")
}

// ===== Constructors =====

impl JsonValue {
    /// Constructs null
    pub const fn null() -> Self {
        JsonValue::Null
    }

    /// Constructs a boolean
    pub const fn bool(b: bool) -> Self {
        JsonValue::Bool(b)
    }

    /// Constructs an integer
    pub const fn integer(i: i64) -> Self {
        JsonValue::Integer(i)
    }

    /// Constructs an unsigned integer (full-range u64 channel)
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let v = JsonValue::u64(u64::MAX - 1);
    /// assert!(v.is_u64());
    /// assert_eq!(v.as_u64(), Some(u64::MAX - 1));
    /// // Form-strict: U64 and Integer are never cross-form equal
    /// assert_ne!(JsonValue::u64(5), JsonValue::Integer(5));
    /// ```
    pub const fn u64(v: u64) -> Self {
        JsonValue::U64(v)
    }

    /// Constructs a string
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let v = JsonValue::string("hello");
    /// assert_eq!(v.as_str(), Some("hello"));
    /// ```
    pub fn string<S: Into<String>>(s: S) -> Self {
        JsonValue::String(Cow::Owned(s.into()))
    }

    /// Constructs an array
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// assert!(arr.is_array());
    /// assert_eq!(arr.as_array().map(|a| a.len()), Some(2));
    /// ```
    pub fn array(v: Vec<JsonValue>) -> Self {
        JsonValue::Array(v)
    }

    /// Constructs an object
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate alloc;
    /// use evorule_tcb::JsonValue;
    /// use alloc::collections::BTreeMap;
    ///
    /// let mut map = BTreeMap::new();
    /// map.insert("key".to_string(), JsonValue::Integer(42));
    /// let obj = JsonValue::object(map);
    /// assert!(obj.is_object());
    /// assert_eq!(obj.get("key").and_then(|v| v.as_i64()), Some(42));
    /// ```
    pub fn object(map: ObjectMap) -> Self {
        JsonValue::Object(map)
    }

    /// Constructs an object from key-value pairs (ownership-taking version)
    ///
    /// More efficient than `object_from_pairs` because values are not cloned.
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let pairs = vec![
    ///     ("a".to_string(), JsonValue::Integer(1)),
    ///     ("b".to_string(), JsonValue::Integer(2)),
    /// ];
    /// let obj = JsonValue::object_from_pairs_owned(pairs);
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// ```
    pub fn object_from_pairs_owned(pairs: Vec<(String, JsonValue)>) -> Self {
        let mut map = ObjectMap::new();
        for (k, v) in pairs {
            map.insert(k, v);
        }
        JsonValue::Object(map)
    }

    /// Constructs an object from key-value pairs (borrowing version, clones values)
    ///
    /// Note: this method clones all values. Use `object_from_pairs_owned` to avoid
    /// cloning.
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let obj = JsonValue::object_from_pairs(&[
    ///     ("a", JsonValue::Integer(1)),
    ///     ("b", JsonValue::Integer(2)),
    /// ]);
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// ```
    pub fn object_from_pairs(pairs: &[(&str, JsonValue)]) -> Self {
        let mut map = ObjectMap::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), v.clone());
        }
        JsonValue::Object(map)
    }

    /// Constructs an empty object
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let obj = JsonValue::empty_object();
    /// assert!(obj.is_object());
    /// assert_eq!(obj.as_object().map(|m| m.len()), Some(0));
    /// ```
    pub fn empty_object() -> Self {
        JsonValue::Object(ObjectMap::new())
    }

    /// Constructs an empty array
    ///
    /// # Examples
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::empty_array();
    /// assert!(arr.is_array());
    /// assert_eq!(arr.as_array().map(|a| a.len()), Some(0));
    /// ```
    pub fn empty_array() -> Self {
        JsonValue::Array(Vec::new())
    }
}

// ===== From impls =====

/// Convenience From impl: object from BTreeMap
impl From<ObjectMap> for JsonValue {
    fn from(map: ObjectMap) -> Self {
        JsonValue::Object(map)
    }
}

/// Convenience From impl: array from Vec
impl From<Vec<JsonValue>> for JsonValue {
    fn from(v: Vec<JsonValue>) -> Self {
        JsonValue::Array(v)
    }
}

/// Convenience From impl: string from &str
impl From<&str> for JsonValue {
    fn from(s: &str) -> Self {
        JsonValue::String(Cow::Owned(s.to_string()))
    }
}

/// Convenience From impl: string from String
impl From<String> for JsonValue {
    fn from(s: String) -> Self {
        JsonValue::String(Cow::Owned(s))
    }
}

/// Convenience From impl: string from Cow<'static, str>
impl From<Cow<'static, str>> for JsonValue {
    fn from(s: Cow<'static, str>) -> Self {
        JsonValue::String(s)
    }
}

/// Convenience From impl: integer from i64
impl From<i64> for JsonValue {
    fn from(i: i64) -> Self {
        JsonValue::Integer(i)
    }
}

/// Convenience From impl: unsigned integer from u64 (full-range u64 channel, no i64 cast wraparound)
impl From<u64> for JsonValue {
    fn from(v: u64) -> Self {
        JsonValue::U64(v)
    }
}

/// Convenience From impl: boolean from bool
impl From<bool> for JsonValue {
    fn from(b: bool) -> Self {
        JsonValue::Bool(b)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use alloc::format;
    use alloc::vec;
    use core::cmp::Ordering;

    // ===== Constructor tests =====

    #[test]
    fn test_const_constructors() {
        const N: JsonValue = JsonValue::null();
        const B: JsonValue = JsonValue::bool(true);
        const I: JsonValue = JsonValue::integer(-7);
        assert_eq!(N, JsonValue::Null);
        assert_eq!(B, JsonValue::Bool(true));
        assert_eq!(I, JsonValue::Integer(-7));
    }

    #[test]
    fn test_string_constructor() {
        let s = JsonValue::string("hello");
        assert_eq!(s.as_str(), Some("hello"));
        let owned = JsonValue::string(String::from("world"));
        assert_eq!(owned.as_str(), Some("world"));
    }

    #[test]
    fn test_empty_array() {
        assert_eq!(JsonValue::empty_array(), JsonValue::Array(Vec::new()));
    }

    #[test]
    fn test_empty_object() {
        assert!(JsonValue::empty_object().is_object());
        assert_eq!(JsonValue::empty_object().len(), Some(0));
    }

    #[test]
    fn test_object_from_pairs_owned() {
        let pairs = vec![
            ("a".to_string(), JsonValue::Integer(1)),
            ("b".to_string(), JsonValue::Integer(2)),
        ];
        let obj = JsonValue::object_from_pairs_owned(pairs);
        assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(obj.get("b").and_then(|v| v.as_i64()), Some(2));
    }

    // ===== Type check tests =====

    #[test]
    fn test_is_methods_match_correct_type() {
        assert!(JsonValue::Null.is_null());
        assert!(JsonValue::Bool(true).is_bool());
        assert!(JsonValue::Integer(0).is_integer());
        assert!(JsonValue::string("x").is_string());
        assert!(JsonValue::empty_array().is_array());
        assert!(JsonValue::empty_object().is_object());
    }

    #[test]
    fn test_is_methods_reject_wrong_type() {
        assert!(!JsonValue::Null.is_bool());
        assert!(!JsonValue::Null.is_integer());
        assert!(!JsonValue::Integer(1).is_string());
        assert!(!JsonValue::string("x").is_array());
        assert!(!JsonValue::empty_array().is_object());
        assert!(!JsonValue::Bool(false).is_null());
    }

    // ===== len() and is_empty() tests =====

    #[test]
    fn test_len_and_is_empty() {
        assert_eq!(JsonValue::Null.len(), None);
        assert_eq!(JsonValue::Integer(42).len(), None);
        assert_eq!(JsonValue::string("test").len(), None);

        let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        assert_eq!(arr.len(), Some(2));
        assert!(!arr.is_empty());

        let empty_arr = JsonValue::empty_array();
        assert_eq!(empty_arr.len(), Some(0));
        assert!(empty_arr.is_empty());

        let obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        assert_eq!(obj.len(), Some(1));
        assert!(!obj.is_empty());

        assert!(JsonValue::empty_object().is_empty());
    }

    // ===== Type conversion tests =====

    #[test]
    fn test_as_methods_success_paths() {
        assert_eq!(JsonValue::Integer(42).as_i64(), Some(42));
        assert_eq!(JsonValue::string("hi").as_str(), Some("hi"));
        assert_eq!(JsonValue::Bool(true).as_bool(), Some(true));
        assert!(JsonValue::empty_array().as_array().is_some());
        assert!(JsonValue::empty_object().as_object().is_some());
    }

    #[test]
    fn test_as_methods_type_mismatch_returns_none() {
        assert_eq!(JsonValue::Null.as_i64(), None);
        assert_eq!(JsonValue::Bool(true).as_i64(), None);
        assert_eq!(JsonValue::string("x").as_i64(), None);
        assert_eq!(JsonValue::Null.as_str(), None);
        assert_eq!(JsonValue::Integer(1).as_str(), None);
        assert_eq!(JsonValue::Integer(1).as_bool(), None);
        assert_eq!(JsonValue::Null.as_array(), None);
        assert_eq!(JsonValue::Integer(0).as_array(), None);
        assert_eq!(JsonValue::Null.as_object(), None);
        assert_eq!(JsonValue::Integer(0).as_object(), None);
    }

    #[test]
    fn test_as_mut_methods_type_mismatch() {
        let mut v = JsonValue::Integer(0);
        assert!(v.as_array_mut().is_none());
        assert!(v.as_object_mut().is_none());
        let mut arr = JsonValue::empty_array();
        assert!(arr.as_array_mut().is_some());
        let mut obj = JsonValue::empty_object();
        assert!(obj.as_object_mut().is_some());
    }

    // ===== Object operation tests =====

    #[test]
    fn test_object_accessors_on_non_object() {
        let mut v = JsonValue::Integer(0);
        assert_eq!(v.get("any"), None);
        assert_eq!(v.get_mut("any"), None);
        assert_eq!(v.insert("k".to_string(), JsonValue::Null), None);
        assert_eq!(v.remove("k"), None);
        assert_eq!(v, JsonValue::Integer(0));
    }

    #[test]
    fn test_get_on_object() {
        let obj = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(7))]);
        assert_eq!(obj.get("x"), Some(&JsonValue::Integer(7)));
        assert_eq!(obj.get("missing"), None);
    }

    #[test]
    fn test_get_mut_on_object() {
        let mut obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        if let Some(v) = obj.get_mut("a") {
            *v = JsonValue::Integer(42);
        }
        assert_eq!(obj.get("a"), Some(&JsonValue::Integer(42)));
        assert_eq!(obj.get_mut("missing"), None);
    }

    #[test]
    fn test_insert_and_remove() {
        let mut obj = JsonValue::empty_object();
        assert_eq!(obj.insert("k".to_string(), JsonValue::Integer(1)), None);
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(1)));

        let old = obj.insert("k".to_string(), JsonValue::Integer(2));
        assert_eq!(old, Some(JsonValue::Integer(1)));
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(2)));

        assert_eq!(obj.remove("k"), Some(JsonValue::Integer(2)));
        assert_eq!(obj.get("k"), None);
        assert_eq!(obj.remove("missing"), None);
    }

    #[test]
    fn test_try_insert() {
        let mut obj = JsonValue::empty_object();
        assert!(obj
            .try_insert("k".to_string(), JsonValue::Integer(1))
            .is_ok());
        assert!(obj
            .try_insert("k".to_string(), JsonValue::Integer(2))
            .is_err());
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(1)));

        let mut non_obj = JsonValue::Integer(0);
        assert!(non_obj
            .try_insert("k".to_string(), JsonValue::Integer(1))
            .is_err());
    }

    #[test]
    fn test_clear() {
        let mut arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        arr.clear();
        assert_eq!(arr.len(), Some(0));

        let mut obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        obj.clear();
        assert_eq!(obj.len(), Some(0));

        let mut null = JsonValue::Null;
        null.clear(); // no-op
        assert!(null.is_null());
    }

    // ===== PartialEq tests =====

    #[test]
    fn test_partial_eq_same_type() {
        assert_eq!(JsonValue::Null, JsonValue::Null);
        assert_eq!(JsonValue::Bool(true), JsonValue::Bool(true));
        assert_eq!(JsonValue::Integer(5), JsonValue::Integer(5));
        assert_eq!(JsonValue::string("hi"), JsonValue::string("hi"));
        assert_eq!(
            JsonValue::array(vec![JsonValue::Integer(1)]),
            JsonValue::array(vec![JsonValue::Integer(1)])
        );
    }

    #[test]
    fn test_partial_eq_cross_type() {
        assert_ne!(JsonValue::Null, JsonValue::Bool(false));
        assert_ne!(JsonValue::Bool(true), JsonValue::Integer(1));
        assert_ne!(JsonValue::Integer(0), JsonValue::string("0"));
        assert_ne!(JsonValue::string(""), JsonValue::empty_array());
        assert_ne!(JsonValue::empty_array(), JsonValue::empty_object());
    }

    #[test]
    fn test_partial_eq_object() {
        let mut a = ObjectMap::new();
        a.insert("x".to_string(), JsonValue::Integer(1));
        a.insert("y".to_string(), JsonValue::Integer(2));
        let mut b = ObjectMap::new();
        b.insert("x".to_string(), JsonValue::Integer(1));
        b.insert("y".to_string(), JsonValue::Integer(2));
        assert_eq!(JsonValue::object(a.clone()), JsonValue::object(b));

        let mut c = ObjectMap::new();
        c.insert("x".to_string(), JsonValue::Integer(1));
        c.insert("y".to_string(), JsonValue::Integer(99));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(c));

        let mut d = ObjectMap::new();
        d.insert("x".to_string(), JsonValue::Integer(1));
        d.insert("z".to_string(), JsonValue::Integer(2));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(d));

        let mut e = ObjectMap::new();
        e.insert("x".to_string(), JsonValue::Integer(1));
        assert_ne!(JsonValue::object(a), JsonValue::object(e));
    }

    // ===== Ord tests =====

    #[test]
    fn test_ord_type_order() {
        assert_eq!(JsonValue::Null.cmp(&JsonValue::Null), Ordering::Equal);
        assert!(JsonValue::Null < JsonValue::Bool(false));
        assert!(JsonValue::Bool(false) < JsonValue::Integer(0));
        assert!(JsonValue::Integer(0) < JsonValue::string(""));
        assert!(JsonValue::string("") < JsonValue::empty_array());
        assert!(JsonValue::empty_array() < JsonValue::empty_object());
    }

    #[test]
    fn test_ord_lexicographic() {
        assert!(JsonValue::Integer(0) < JsonValue::Integer(1));
        assert!(JsonValue::string("a") < JsonValue::string("b"));

        let mut short = ObjectMap::new();
        short.insert("a".to_string(), JsonValue::Integer(1));
        let mut long = ObjectMap::new();
        long.insert("a".to_string(), JsonValue::Integer(1));
        long.insert("b".to_string(), JsonValue::Integer(2));
        assert!(JsonValue::object(short) < JsonValue::object(long));
    }

    // ===== Display tests =====

    #[test]
    fn test_display_simple_types() {
        assert_eq!(format!("{}", JsonValue::Null), "null");
        assert_eq!(format!("{}", JsonValue::Bool(true)), "true");
        assert_eq!(format!("{}", JsonValue::Bool(false)), "false");
        assert_eq!(format!("{}", JsonValue::Integer(0)), "0");
        assert_eq!(format!("{}", JsonValue::Integer(-42)), "-42");
    }

    #[test]
    fn test_display_string_escapes() {
        assert_eq!(format!("{}", JsonValue::string("")), "\"\"");
        assert_eq!(format!("{}", JsonValue::string("hello")), "\"hello\"");
        assert_eq!(format!("{}", JsonValue::string("a\"b")), "\"a\\\"b\"");
        assert_eq!(format!("{}", JsonValue::string("a\\b")), "\"a\\\\b\"");
        assert_eq!(format!("{}", JsonValue::string("a\nb")), "\"a\\nb\"");
    }

    #[test]
    fn test_display_array() {
        assert_eq!(format!("{}", JsonValue::empty_array()), "[]");
        let arr = JsonValue::array(vec![
            JsonValue::Integer(1),
            JsonValue::Integer(2),
            JsonValue::Integer(3),
        ]);
        assert_eq!(format!("{arr}"), "[1, 2, 3]");
    }

    #[test]
    fn test_display_object() {
        assert_eq!(format!("{}", JsonValue::empty_object()), "{}");
        let obj = JsonValue::object_from_pairs(&[
            ("z", JsonValue::Integer(1)),
            ("a", JsonValue::Integer(2)),
        ]);
        assert_eq!(format!("{obj}"), "{\"a\": 2, \"z\": 1}");
    }

    // ===== From trait tests =====

    #[test]
    fn test_from_traits() {
        let v: JsonValue = 42i64.into();
        assert_eq!(v, JsonValue::Integer(42));

        let v: JsonValue = "hello".into();
        assert_eq!(v, JsonValue::string("hello"));

        let v: JsonValue = true.into();
        assert_eq!(v, JsonValue::Bool(true));

        let v: JsonValue = vec![JsonValue::Integer(1), JsonValue::Integer(2)].into();
        assert_eq!(v.len(), Some(2));
    }

    // ===== JSON string escaping boundaries (Display output validity) =====

    /// Control characters (< 0x20 and not dedicated escape chars) must go through
    /// the \u00XX channel; verbatim output would produce invalid JSON (consumers
    /// crash on parse)
    #[test]
    fn test_display_escapes_control_chars() {
        let v = JsonValue::string("a\u{1}b");
        assert_eq!(v.to_string(), "\"a\\u0001b\"");

        // \x1f is the last non-dedicated control character below the 0x20 boundary
        let v = JsonValue::string("\u{1f}");
        assert_eq!(v.to_string(), "\"\\u001f\"");
    }

    /// Space (0x20) is a printable character, outside the escape range —
    /// writing the escape bound as <= would wrongly escape space as \u0020
    #[test]
    fn test_display_space_verbatim() {
        let v = JsonValue::string("a b");
        assert_eq!(v.to_string(), "\"a b\"");
    }

    // ===== U64 (full-range u64 channel) =====

    #[test]
    fn test_u64_construct_and_access() {
        let v = JsonValue::u64(u64::MAX - 1);
        assert!(v.is_u64());
        assert!(!v.is_integer());
        assert_eq!(v.as_u64(), Some(u64::MAX - 1));
        // Form-strict symmetry: as_i64 only matches Integer, as_u64 only matches U64;
        // is_u64 likewise only matches U64 — Integer/Bool and other non-U64 are
        // always false
        // (negative-kill assertion against an is_u64-always-true mutation,
        // added in an earlier internal batch)
        assert_eq!(v.as_i64(), None);
        assert_eq!(JsonValue::Integer(5).as_u64(), None);
        assert!(!JsonValue::Integer(5).is_u64());
        assert!(!JsonValue::Bool(true).is_u64());
        assert!(!JsonValue::Null.is_u64());

        let from_trait: JsonValue = 42u64.into();
        assert_eq!(from_trait, JsonValue::u64(42));
    }

    #[test]
    fn test_u64_partial_eq_strict_shape() {
        // Cross-form is intentionally unequal (consistent with the Ord type grouping)
        assert_ne!(JsonValue::u64(5), JsonValue::Integer(5));
        assert_ne!(JsonValue::Integer(5), JsonValue::u64(5));
        assert_eq!(JsonValue::u64(5), JsonValue::u64(5));
        assert_ne!(JsonValue::u64(u64::MAX), JsonValue::u64(u64::MAX - 1));
    }

    #[test]
    fn test_u64_ord_group_order() {
        // Group order: Integer < U64 < String (type grouping takes precedence over numeric value)
        assert!(JsonValue::Integer(0) < JsonValue::u64(0));
        assert!(JsonValue::u64(0) < JsonValue::string(""));
        assert!(JsonValue::u64(0) < JsonValue::u64(1));
        assert!(JsonValue::u64(u64::MAX - 1) < JsonValue::u64(u64::MAX));
        // The Integer group still orders below any value in the U64 group (inter-group comparison takes precedence)
        assert!(JsonValue::Integer(i64::MAX) < JsonValue::u64(0));
    }

    #[test]
    fn test_u64_display() {
        assert_eq!(format!("{}", JsonValue::u64(0)), "0");
        assert_eq!(
            format!("{}", JsonValue::u64(u64::MAX - 1)),
            "18446744073709551614"
        );
    }
}
