// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Path resolution - dot-separated segments + array indexes, never panics
//!
//! # Path syntax (ABNF)
//! ```text
//! path           = segment *( "." segment )
//! segment        = identifier [ "[" index "]" ]
//! identifier     = 1*( char - reserved )
//! char-reserved  = "." / "[" / "\"   (`]` outside an index is a plain char)
//! index          = 1*DIGIT
//!
//! Note: identifiers are not charset-restricted (field names may contain any
//! non-reserved character); this is distinct from the strict character set
//! enforced by the `is_root_segment_dot_path` ambiguity predicate.
//! ```
//!
//! # Escaping rules
//! - A `.` inside a field name must be escaped as `\.`
//! - A `[` inside a field name must be escaped as `\[`
//!
//! # Examples
//! - `__exec__.payload.items[0].value` → accesses the `value` field of array element 0
//! - `data\.version` → the field name is `data.version`
//!
//! # Guarantees
//! - Never panics
//! - Any parse failure returns `None`

use crate::value::JsonValue;
use alloc::string::String;
use alloc::vec::Vec;

/// Resolves a path, returning a reference to the value (if present)
///
/// # Examples
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::JsonValue;
/// use evorule_tcb::path::resolve_path;
/// use alloc::collections::BTreeMap;
///
/// // Build a nested state
/// let mut inner = BTreeMap::new();
/// inner.insert("value".to_string(), JsonValue::Integer(42));
/// let mut mid = BTreeMap::new();
/// mid.insert("inner".to_string(), JsonValue::object(inner));
/// let state = JsonValue::object(mid);
///
/// // Path resolution
/// assert_eq!(resolve_path(&state, "inner.value").and_then(|v| v.as_i64()), Some(42));
/// assert_eq!(resolve_path(&state, "missing"), None);
/// assert_eq!(resolve_path(&state, ""), None);
/// ```
pub fn resolve_path<'a>(state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    if path.is_empty() {
        return None;
    }

    // Check for a trailing dot (e.g. "x.")
    if path.ends_with('.') {
        return None;
    }

    let segments = parse_path_segments(path)?;
    let mut current = state;

    for seg in &segments {
        match seg {
            PathSegment::Field(field) => {
                current = match current {
                    JsonValue::Object(map) => map.get(field.as_str())?,
                    _ => return None,
                };
            }
            PathSegment::Index(field, idx) => {
                // Access the field first (if present), then the index
                let target = if let Some(f) = field {
                    match current {
                        JsonValue::Object(map) => map.get(f.as_str())?,
                        _ => return None,
                    }
                } else {
                    current
                };
                current = match target {
                    JsonValue::Array(arr) => arr.get(*idx)?,
                    _ => return None,
                };
            }
        }
    }

    Some(current)
}

/// Resolves a path, returning a mutable reference to the value (if present)
pub fn resolve_path_mut<'a>(state: &'a mut JsonValue, path: &str) -> Option<&'a mut JsonValue> {
    if path.is_empty() {
        return None;
    }

    if path.ends_with('.') {
        return None;
    }

    let segments = parse_path_segments(path)?;
    resolve_path_mut_inner(state, &segments)
}

/// Resolves a path relative to the `__exec__` context (unified path convention, since v0.3.2)
///
/// The domain `path` uses this function (`__exec__`-relative paths are the
/// engine-wide unified convention):
///
/// - Prefix `__exec__.`: the prefix is stripped and the path is resolved from the
///   `__exec__` node (absolute-path compatible form)
/// - Other forms: the `__exec__.` prefix is prepended automatically (relative
///   paths, e.g. `payload.x`, `instruction.type`, `queue[0].type`)
///
/// Resolution failure returns `None` (the caller decides the evaluation semantics:
/// domains treat it as false; state-mutating instructions turn it into an explicit
/// `PathResolutionFailed`).
pub(crate) fn resolve_exec_path<'a>(state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let stripped = path.strip_prefix("__exec__.").unwrap_or(path);
    let exec = state.get("__exec__")?;
    resolve_path(exec, stripped)
}

/// Internal recursive implementation (mutable path resolution)
fn resolve_path_mut_inner<'a>(
    state: &'a mut JsonValue,
    segments: &[PathSegment],
) -> Option<&'a mut JsonValue> {
    // Empty segment list: return the current state (recursion termination)
    if segments.is_empty() {
        return Some(state);
    }

    // Use split_first to avoid indexing_slicing (never panics)
    let (first, rest) = segments.split_first()?;

    match first {
        PathSegment::Field(field) => {
            let obj = state.as_object_mut()?;
            let value = obj.get_mut(field.as_str())?;
            resolve_path_mut_inner(value, rest)
        }
        PathSegment::Index(field, idx) => {
            let target = if let Some(f) = field {
                let obj = state.as_object_mut()?;
                obj.get_mut(f.as_str())?
            } else {
                state
            };
            let arr = target.as_array_mut()?;
            let value = arr.get_mut(*idx)?;
            resolve_path_mut_inner(value, rest)
        }
    }
}

/// Path segment (parsed intermediate representation)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathSegment {
    /// Field access (e.g. `payload`)
    Field(String),
    /// Index access (e.g. `items[0]` or `[0]`; a `None` field means a bare index)
    Index(Option<String>, usize),
}

/// Parses a path string into a segment list
///
/// Syntax constraints:
/// - Field names are required on both sides of a dot separator; the only exception
///   is the **first** dot after `]` (both `items[0].name` and `data.[0]` are
///   accepted as equivalent forms);
/// - Consecutive dots after an index segment (e.g. `items[0]..name`) are illegal.
pub(crate) fn parse_path_segments(path: &str) -> Option<Vec<PathSegment>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    let mut escaped = false;
    // Whether the previously processed character was the ']' of an index segment:
    // only the first dot after it may leave current empty (reset immediately after
    // consumption; consecutive dots are rejected)
    let mut just_closed_index = false;

    while let Some(c) = chars.next() {
        if escaped {
            // Escaped character: keep verbatim
            current.push(c);
            just_closed_index = false;
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            continue;
        }

        if c == '.' {
            // Dot separator: an empty current is legal only right after a closed index segment
            let after_index = just_closed_index;
            just_closed_index = false;
            if current.is_empty() && !after_index {
                // A true empty segment (e.g. "x..y", ".x" or "x[0]..y"), illegal
                return None;
            }
            if !current.is_empty() {
                segments.push(PathSegment::Field(core::mem::take(&mut current)));
            }
            continue;
        }

        if c == '[' {
            // Index begins
            let field = if current.is_empty() {
                None
            } else {
                Some(core::mem::take(&mut current))
            };
            // Parse the index up to ']' and verify the closing bracket exists
            let mut idx_str = String::new();
            let mut found_close = false;
            for ic in chars.by_ref() {
                if ic == ']' {
                    found_close = true;
                    break;
                }
                idx_str.push(ic);
            }
            if !found_close {
                // Missing closing ']' (e.g. "[0" or "items[0"), illegal
                return None;
            }
            if idx_str.is_empty() {
                return None;
            }
            let idx: usize = idx_str.parse().ok()?;
            segments.push(PathSegment::Index(field, idx));
            just_closed_index = true;
            // After ']' must come '.', '[' or end of string (rejects illegal splices like "[0]abc")
            match chars.peek() {
                None | Some('.' | '[') => {}
                _ => return None,
            }
            continue;
        }

        // Ordinary character
        current.push(c);
        just_closed_index = false;
    }

    // Handle the tail
    if escaped {
        // Unterminated escape (e.g. "x\. "), illegal
        return None;
    }
    // A path ending with a dot (e.g. "x.") is illegal syntax: a dot is a separator
    // and must be followed by another segment.
    // current is necessarily empty here (the dot was consumed earlier and cleared
    // current), but segments may be non-empty; without this check the path would be
    // silently accepted as a prefix path, masking typos.
    if current.is_empty() && !just_closed_index {
        return None;
    }
    if !current.is_empty() {
        segments.push(PathSegment::Field(current));
    }

    if segments.is_empty() {
        return None;
    }

    Some(segments)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::value::{JsonValue, ObjectMap};
    use alloc::string::ToString;
    use alloc::vec;

    // ===== Basic path resolution tests =====

    #[test]
    fn test_resolve_path_simple() {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "x"), Some(&JsonValue::Integer(42)));
        assert_eq!(resolve_path(&state, "y"), None);
        assert_eq!(resolve_path(&state, ""), None);
        assert_eq!(resolve_path(&state, "x."), None);
    }

    #[test]
    fn test_resolve_path_nested() {
        let mut inner = ObjectMap::new();
        inner.insert("value".to_string(), JsonValue::Integer(99));
        let mut outer = ObjectMap::new();
        outer.insert("data".to_string(), JsonValue::Object(inner));
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "data.value"),
            Some(&JsonValue::Integer(99))
        );
        assert_eq!(resolve_path(&state, "data.missing"), None);
    }

    #[test]
    fn test_resolve_path_array_index() {
        let arr = JsonValue::array(vec![
            JsonValue::Integer(10),
            JsonValue::Integer(20),
            JsonValue::Integer(30),
        ]);
        let state = arr;

        assert_eq!(resolve_path(&state, "[0]"), Some(&JsonValue::Integer(10)));
        assert_eq!(resolve_path(&state, "[2]"), Some(&JsonValue::Integer(30)));
        assert_eq!(resolve_path(&state, "[5]"), None);
        assert_eq!(resolve_path(&state, "[-1]"), None);
    }

    #[test]
    fn test_resolve_path_mixed() {
        let mut item1 = ObjectMap::new();
        item1.insert("name".to_string(), JsonValue::string("alpha"));
        let mut item2 = ObjectMap::new();
        item2.insert("name".to_string(), JsonValue::string("beta"));
        let items = JsonValue::array(vec![JsonValue::Object(item1), JsonValue::Object(item2)]);
        let mut outer = ObjectMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "items[0].name"),
            Some(&JsonValue::string("alpha"))
        );
        assert_eq!(
            resolve_path(&state, "items[1].name"),
            Some(&JsonValue::string("beta"))
        );
        assert_eq!(resolve_path(&state, "items[2].name"), None);
    }

    #[test]
    fn test_resolve_path_escaped() {
        let mut map = ObjectMap::new();
        map.insert("data.version".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "data\\.version"),
            Some(&JsonValue::Integer(42))
        );
        assert_eq!(resolve_path(&state, "data.version"), None);
    }

    // ===== resolve_path_mut tests =====

    #[test]
    fn test_resolve_path_mut_basic() {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let mut state = JsonValue::Object(map);

        if let Some(value) = resolve_path_mut(&mut state, "x") {
            *value = JsonValue::Integer(100);
        }
        assert_eq!(resolve_path(&state, "x"), Some(&JsonValue::Integer(100)));
    }

    #[test]
    fn test_resolve_path_mut_nested() {
        let mut inner = ObjectMap::new();
        inner.insert("value".to_string(), JsonValue::Integer(1));
        let mut outer = ObjectMap::new();
        outer.insert("data".to_string(), JsonValue::Object(inner));
        let mut state = JsonValue::Object(outer);

        if let Some(value) = resolve_path_mut(&mut state, "data.value") {
            *value = JsonValue::Integer(99);
        }
        assert_eq!(
            resolve_path(&state, "data.value"),
            Some(&JsonValue::Integer(99))
        );
    }

    #[test]
    fn test_resolve_path_mut_array_element() {
        let mut state = JsonValue::array(vec![
            JsonValue::Integer(10),
            JsonValue::Integer(20),
            JsonValue::Integer(30),
        ]);

        if let Some(value) = resolve_path_mut(&mut state, "[1]") {
            *value = JsonValue::Integer(200);
        }
        assert_eq!(resolve_path(&state, "[1]"), Some(&JsonValue::Integer(200)));
    }

    #[test]
    fn test_resolve_path_mut_nonexistent_returns_none() {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let mut state = JsonValue::Object(map);

        assert!(resolve_path_mut(&mut state, "y").is_none());
        assert!(resolve_path_mut(&mut state, "").is_none());
        assert!(resolve_path_mut(&mut state, "x.").is_none());
    }

    // ===== Edge case tests =====

    #[test]
    fn test_resolve_path_special_chars_in_field_name() {
        let mut map = ObjectMap::new();
        map.insert("field_name".to_string(), JsonValue::Integer(1));
        map.insert("field-name".to_string(), JsonValue::Integer(2));
        map.insert("__exec__".to_string(), JsonValue::Integer(3));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "field_name"),
            Some(&JsonValue::Integer(1))
        );
        assert_eq!(
            resolve_path(&state, "field-name"),
            Some(&JsonValue::Integer(2))
        );
        assert_eq!(
            resolve_path(&state, "__exec__"),
            Some(&JsonValue::Integer(3))
        );
    }

    #[test]
    fn test_resolve_path_empty_index_returns_none() {
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[]"), None);
    }

    #[test]
    fn test_resolve_path_invalid_index_returns_none() {
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[abc]"), None);
    }

    #[test]
    fn test_resolve_path_double_dot_returns_none() {
        let mut inner = ObjectMap::new();
        inner.insert("y".to_string(), JsonValue::Integer(1));
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Object(inner));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "x..y"), None);
    }

    #[test]
    fn test_resolve_path_consecutive_dots_after_index_returns_none() {
        // L1 regression: only one dot is allowed after an index segment; consecutive dots are illegal
        let mut inner = ObjectMap::new();
        inner.insert("name".to_string(), JsonValue::string("alpha"));
        let items = JsonValue::array(vec![JsonValue::Object(inner)]);
        let mut outer = ObjectMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(resolve_path(&state, "items[0]..name"), None);
        assert_eq!(resolve_path(&state, "items[0]...name"), None);
        // Same for a leading bare index
        let arr = JsonValue::array(vec![JsonValue::array(vec![JsonValue::Integer(1)])]);
        assert_eq!(resolve_path(&arr, "[0]..x"), None);
        // Positive control: a single dot still works
        assert_eq!(
            resolve_path(&state, "items[0].name"),
            Some(&JsonValue::string("alpha"))
        );
    }

    #[test]
    fn test_resolve_path_leading_dot_returns_none() {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(1));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, ".x"), None);
    }

    #[test]
    fn test_resolve_path_on_non_object_returns_none() {
        let state = JsonValue::Integer(42);
        assert_eq!(resolve_path(&state, "x"), None);
        assert_eq!(resolve_path(&state, "[0]"), None);
    }

    #[test]
    fn test_resolve_path_on_non_array_returns_none() {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(1));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "[0]"), None);
    }

    #[test]
    fn test_resolve_path_escaped_bracket() {
        let mut map = ObjectMap::new();
        map.insert("data[0]".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "data\\[0]"),
            Some(&JsonValue::Integer(42))
        );
    }

    #[test]
    fn test_resolve_path_missing_closing_bracket_returns_none() {
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[0"), None);
    }

    #[test]
    fn test_resolve_path_missing_closing_bracket_with_field_returns_none() {
        let mut inner = ObjectMap::new();
        inner.insert("name".to_string(), JsonValue::string("alpha"));
        let items = JsonValue::array(vec![JsonValue::Object(inner)]);
        let mut outer = ObjectMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(resolve_path(&state, "items[0"), None);
    }

    #[test]
    fn test_resolve_path_no_separator_after_bracket_returns_none() {
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[0]abc"), None);
    }

    #[test]
    fn test_resolve_path_no_separator_after_bracket_with_field_returns_none() {
        let mut inner = ObjectMap::new();
        inner.insert("name".to_string(), JsonValue::string("alpha"));
        let items = JsonValue::array(vec![JsonValue::Object(inner)]);
        let mut outer = ObjectMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(resolve_path(&state, "items[0]name"), None);
    }

    #[test]
    fn test_resolve_path_nested_array_indexing() {
        let row0 = JsonValue::array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]);
        let row1 = JsonValue::array(vec![JsonValue::Integer(30), JsonValue::Integer(40)]);
        let matrix = JsonValue::array(vec![row0, row1]);
        let mut outer = ObjectMap::new();
        outer.insert("matrix".to_string(), matrix);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "matrix[0][1]"),
            Some(&JsonValue::Integer(20))
        );
        assert_eq!(
            resolve_path(&state, "matrix[1][0]"),
            Some(&JsonValue::Integer(30))
        );
    }

    #[test]
    fn test_resolve_path_dot_then_bracket_works() {
        let arr = JsonValue::array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]);
        let mut outer = ObjectMap::new();
        outer.insert("data".to_string(), arr);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "data.[0]"),
            Some(&JsonValue::Integer(10))
        );
        assert_eq!(
            resolve_path(&state, "data[0]"),
            Some(&JsonValue::Integer(10))
        );
    }

    #[test]
    fn test_resolve_path_index_with_field_on_non_object_returns_none() {
        let state = JsonValue::array(vec![JsonValue::Integer(42)]);
        assert_eq!(resolve_path(&state, "a[0]"), None);
    }

    #[test]
    fn test_resolve_path_mut_index_with_field_on_non_object_returns_none() {
        let mut state = JsonValue::array(vec![JsonValue::Integer(42)]);
        assert_eq!(resolve_path_mut(&mut state, "a[0]"), None);
    }

    #[test]
    fn test_resolve_path_mut_index_with_missing_field_on_object_returns_none() {
        let mut state = JsonValue::Object(ObjectMap::new());
        assert_eq!(resolve_path_mut(&mut state, "a[0]"), None);
    }

    #[test]
    fn test_resolve_path_trailing_backslash_returns_none() {
        let state = JsonValue::Object(ObjectMap::new());
        assert_eq!(resolve_path(&state, "x\\"), None);
    }

    #[test]
    fn test_resolve_path_only_backslash_returns_none() {
        let state = JsonValue::Object(ObjectMap::new());
        assert_eq!(resolve_path(&state, "\\"), None);
    }

    #[test]
    fn test_parse_path_segments_empty_returns_none() {
        assert_eq!(parse_path_segments(""), None);
    }

    // ===== Extra tests: dot followed by an index =====

    #[test]
    fn test_resolve_path_dot_before_bracket_works() {
        let arr = JsonValue::array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]);
        let mut outer = ObjectMap::new();
        outer.insert("data".to_string(), arr);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "data.[0]"),
            Some(&JsonValue::Integer(10))
        );
        assert_eq!(
            resolve_path(&state, "data.[1]"),
            Some(&JsonValue::Integer(20))
        );
    }
}
