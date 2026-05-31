//! Metadata: a self-describing key→value map carried on the wire as a dynamic
//! [`Value`] (`r[rpc.metadata]`). Values are strings, byte runs, or `u64`s.
//!
//! There are no duplicate keys (a later write for a key replaces the earlier one).
//! Per-key behavior flags (sensitive, do-not-propagate) are not per-entry flags any
//! more — they are recorded under the well-known keys in [`keys`], each holding an
//! array of the metadata key-names the flag applies to.
//!
//! Build metadata with the fluent [`metadata`] builder; read it through the
//! [`MetadataExt`] accessors. Construction leans on `Default` — an absent metadata
//! field is just `Value::default()` (null), which reads as empty.

use facet_value::{VArray, VBytes, VObject, VString, Value};

/// Metadata is a self-describing [`Value`] — an object of string keys to values
/// (string / bytes / `u64`), or null when empty.
pub type Metadata = Value;

/// Well-known metadata keys carrying per-key behavior flags. Each holds an array of
/// the metadata key-names the flag applies to (`r[rpc.metadata.flags]`).
pub mod keys {
    /// Keys whose values MUST NOT be logged, traced, or included in error messages
    /// (`r[rpc.metadata.flags.sensitive]`).
    pub const SENSITIVE: &str = "vox:sensitive";

    /// Keys whose values MUST NOT be forwarded to downstream calls
    /// (`r[rpc.metadata.flags.no-propagate]`).
    pub const NO_PROPAGATE: &str = "vox:no-propagate";
}

/// Per-key behavior flags, used only as a **construction-time convenience** — they
/// are NOT a wire type. On insert, set flags are recorded under the well-known
/// [`keys`] (`r[rpc.metadata.flags]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetadataFlags(u64);

impl MetadataFlags {
    /// No special handling.
    pub const NONE: Self = Self(0);
    /// Value MUST NOT be logged, traced, or included in error messages.
    pub const SENSITIVE: Self = Self(1 << 0);
    /// Value MUST NOT be forwarded to downstream calls.
    pub const NO_PROPAGATE: Self = Self(1 << 1);

    /// Returns `true` if all flags in `other` are set in `self`.
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for MetadataFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for MetadataFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Insert (or replace) `key`→`value` into a metadata [`Value`], creating the object
/// if needed, and record any [`MetadataFlags`] under the well-known [`keys`]. The
/// construction primitive the builder and the middleware push-helpers share.
pub fn meta_set(metadata: &mut Metadata, key: &str, value: impl Into<Value>, flags: MetadataFlags) {
    if metadata.as_object().is_none() {
        *metadata = Value::from(VObject::new());
    }
    let obj = metadata
        .as_object_mut()
        .expect("metadata was just made an object");
    obj.insert(VString::new(key), value.into());
    if flags.contains(MetadataFlags::SENSITIVE) {
        append_flag(metadata, keys::SENSITIVE, key);
    }
    if flags.contains(MetadataFlags::NO_PROPAGATE) {
        append_flag(metadata, keys::NO_PROPAGATE, key);
    }
}

/// Append `key` to the string array under the well-known `flag_key`.
fn append_flag(metadata: &mut Metadata, flag_key: &str, key: &str) {
    let Some(obj) = metadata.as_object_mut() else {
        return;
    };
    if let Some(arr) = obj.get_mut(flag_key).and_then(Value::as_array_mut) {
        arr.push(Value::from(VString::new(key)));
    } else {
        let mut arr = VArray::new();
        arr.push(Value::from(VString::new(key)));
        obj.insert(VString::new(flag_key), Value::from(arr));
    }
}

/// Start building metadata fluently: `metadata().str("trace", "abc").u64("n", 5).build()`.
#[must_use]
pub fn metadata() -> MetadataBuilder {
    MetadataBuilder {
        obj: VObject::new(),
    }
}

/// Fluent builder producing a metadata [`Value`] object.
pub struct MetadataBuilder {
    obj: VObject,
}

impl MetadataBuilder {
    /// Add (or replace) a string entry.
    #[must_use]
    pub fn str(mut self, key: impl Into<VString>, value: impl Into<VString>) -> Self {
        self.obj.insert(key, Value::from(value.into()));
        self
    }

    /// Add (or replace) a `u64` entry.
    #[must_use]
    pub fn u64(mut self, key: impl Into<VString>, value: u64) -> Self {
        self.obj.insert(key, Value::from(value));
        self
    }

    /// Add (or replace) a byte-run entry.
    #[must_use]
    pub fn bytes(mut self, key: impl Into<VString>, value: impl Into<VBytes>) -> Self {
        self.obj.insert(key, Value::from(value.into()));
        self
    }

    /// Mark `key` sensitive (records it under the `vox:sensitive` well-known key).
    #[must_use]
    pub fn sensitive(mut self, key: &str) -> Self {
        self.add_flag(keys::SENSITIVE, key);
        self
    }

    /// Mark `key` do-not-propagate (records it under the `vox:no-propagate` key).
    #[must_use]
    pub fn no_propagate(mut self, key: &str) -> Self {
        self.add_flag(keys::NO_PROPAGATE, key);
        self
    }

    /// Append `key` to the array under the well-known `flag_key`, creating it if absent.
    fn add_flag(&mut self, flag_key: &str, key: &str) {
        if let Some(arr) = self.obj.get_mut(flag_key).and_then(Value::as_array_mut) {
            arr.push(Value::from(VString::new(key)));
        } else {
            let mut arr = VArray::new();
            arr.push(Value::from(VString::new(key)));
            self.obj.insert(flag_key, Value::from(arr));
        }
    }

    /// Finish building, returning the metadata [`Value`].
    #[must_use]
    pub fn build(self) -> Metadata {
        Value::from(self.obj)
    }
}

/// Read accessors and flag queries for a metadata [`Value`]. Implemented for
/// [`Value`]; a null value reads as empty.
pub trait MetadataExt {
    /// The string value at `key`, if present and a string.
    fn meta_str(&self, key: &str) -> Option<&str>;
    /// The `u64` value at `key`, if present and a number.
    fn meta_u64(&self, key: &str) -> Option<u64>;
    /// The byte-run value at `key`, if present and bytes.
    fn meta_bytes(&self, key: &str) -> Option<&[u8]>;
    /// Whether there are no metadata entries.
    fn meta_is_empty(&self) -> bool;
    /// The number of entries (0 when null).
    fn meta_len(&self) -> usize;
    /// Whether `key` is marked sensitive (`r[rpc.metadata.flags.sensitive]`).
    fn meta_is_sensitive(&self, key: &str) -> bool;
    /// Whether `key` is marked do-not-propagate (`r[rpc.metadata.flags.no-propagate]`).
    fn meta_is_no_propagate(&self, key: &str) -> bool;
    /// Iterate the (non-well-known) `(key, value)` entries.
    fn meta_entries(&self) -> Vec<(&str, &Value)>;
}

/// Whether `value` (an array of strings under a well-known flag key) lists `key`.
fn flag_lists(value: Option<&Value>, key: &str) -> bool {
    value.and_then(Value::as_array).is_some_and(|arr| {
        arr.iter()
            .any(|v| v.as_string().map(VString::as_str) == Some(key))
    })
}

impl MetadataExt for Value {
    fn meta_str(&self, key: &str) -> Option<&str> {
        self.as_object()?.get(key)?.as_string().map(VString::as_str)
    }

    fn meta_u64(&self, key: &str) -> Option<u64> {
        self.as_object()?.get(key)?.as_number()?.to_u64()
    }

    fn meta_bytes(&self, key: &str) -> Option<&[u8]> {
        self.as_object()?.get(key)?.as_bytes().map(VBytes::as_slice)
    }

    fn meta_is_empty(&self) -> bool {
        self.meta_len() == 0
    }

    fn meta_len(&self) -> usize {
        self.as_object().map_or(0, VObject::len)
    }

    fn meta_is_sensitive(&self, key: &str) -> bool {
        flag_lists(self.as_object().and_then(|o| o.get(keys::SENSITIVE)), key)
    }

    fn meta_is_no_propagate(&self, key: &str) -> bool {
        flag_lists(
            self.as_object().and_then(|o| o.get(keys::NO_PROPAGATE)),
            key,
        )
    }

    fn meta_entries(&self) -> Vec<(&str, &Value)> {
        match self.as_object() {
            Some(obj) => obj
                .iter()
                .filter(|(k, _)| k.as_str() != keys::SENSITIVE && k.as_str() != keys::NO_PROPAGATE)
                .map(|(k, v)| (k.as_str(), v))
                .collect(),
            None => Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------------
// Compatibility shims (delegate to the builder / accessors)
// ----------------------------------------------------------------------------

/// Look up a string metadata value by key.
pub fn metadata_get_str<'a>(metadata: &'a Metadata, key: &str) -> Option<&'a str> {
    metadata.meta_str(key)
}

/// Look up a `u64` metadata value by key.
pub fn metadata_get_u64(metadata: &Metadata, key: &str) -> Option<u64> {
    metadata.meta_u64(key)
}

/// Metadata is already an owned [`Value`]; conversion is the identity.
#[must_use]
pub fn metadata_into_owned(metadata: Metadata) -> Metadata {
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_and_accessors_round_trip() {
        let m = metadata()
            .str("trace", "abc")
            .u64("n", 99)
            .bytes("blob", &[1u8, 2, 3][..])
            .sensitive("trace")
            .build();

        assert_eq!(m.meta_str("trace"), Some("abc"));
        assert_eq!(m.meta_u64("n"), Some(99));
        assert_eq!(m.meta_bytes("blob"), Some(&[1u8, 2, 3][..]));
        assert!(m.meta_is_sensitive("trace"));
        assert!(!m.meta_is_sensitive("n"));
        // The well-known flag key is hidden from the logical entries.
        let entries: Vec<&str> = m.meta_entries().into_iter().map(|(k, _)| k).collect();
        assert_eq!(entries.len(), 3);
        assert!(!entries.contains(&keys::SENSITIVE));
    }

    #[test]
    fn default_is_empty() {
        let m = Metadata::default();
        assert!(m.meta_is_empty());
        assert_eq!(m.meta_len(), 0);
        assert_eq!(m.meta_str("x"), None);
    }
}
