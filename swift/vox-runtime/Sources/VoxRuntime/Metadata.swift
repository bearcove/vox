import PhonSchema

// Metadata: a self-describing key→value map carried on the wire as a phon dynamic
// `Value` (`r[rpc.metadata]`) — an object of string keys to string / bytes / u64
// values, or null when empty. Mirrors `rust/vox-types/src/metadata.rs`.
// r[impl schema.interaction.metadata]
//
// Per-key behavior flags (sensitive, no-propagate) are recorded under the
// well-known keys below, each holding an array of the metadata key-names the flag
// applies to — they are NOT a per-entry wire field.

/// Metadata is a self-describing [`Value`].
public typealias Metadata = Value

/// Well-known metadata keys carrying per-key behavior flags
/// (`r[rpc.metadata.flags]`).
public enum MetadataKeys {
    /// Keys whose values MUST NOT be logged/traced (`r[rpc.metadata.flags.sensitive]`).
    public static let sensitive = "vox:sensitive"
    /// Keys whose values MUST NOT be forwarded downstream
    /// (`r[rpc.metadata.flags.no-propagate]`).
    public static let noPropagate = "vox:no-propagate"
}

/// Empty metadata reads as null (an absent metadata field).
public func emptyMetadata() -> Metadata { .null }

/// Coerce a decoded value to metadata: anything that is not an object reads as empty.
public func coerceMetadata(_ value: Value) -> Metadata {
    if case .object = value { return value }
    return .null
}

public extension Value {
    /// The string value at `key`, if present and a string.
    func metaStr(_ key: String) -> String? { get(key)?.asString }

    /// The `u64` value at `key`, if present and a non-negative integer that fits.
    func metaU64(_ key: String) -> UInt64? { get(key)?.asNumber?.toU64 }

    /// The byte-run value at `key`, if present and bytes.
    func metaBytes(_ key: String) -> [UInt8]? { get(key)?.asBytes }

    /// Whether there are no metadata entries.
    var metaIsEmpty: Bool { (asObject?.isEmpty) ?? true }

    /// The number of entries (0 when null).
    var metaLen: Int { asObject?.count ?? 0 }

    /// Whether a key is present.
    func metaHas(_ key: String) -> Bool { get(key) != nil }

    /// The (non-well-known) `(key, value)` entries.
    func metaEntries() -> [(String, Value)] {
        guard case .object(let entries) = self else { return [] }
        return entries
            .filter { $0.key != MetadataKeys.sensitive && $0.key != MetadataKeys.noPropagate }
            .map { ($0.key, $0.value) }
    }

    /// Insert (or replace) `key`→`value`, creating the object if needed. The shared
    /// construction primitive (mirrors `meta_set`).
    mutating func metaSet(_ key: String, _ value: Value) {
        var entries: [Entry]
        if case .object(let e) = self { entries = e } else { entries = [] }
        if let i = entries.firstIndex(where: { $0.key == key }) {
            entries[i].value = value
        } else {
            entries.append(Entry(key: key, value: value))
        }
        self = .object(entries)
    }

    /// Remove `key` if present.
    mutating func metaRemove(_ key: String) {
        guard case .object(var entries) = self else { return }
        entries.removeAll { $0.key == key }
        self = entries.isEmpty ? .null : .object(entries)
    }

    /// Functional `metaSet` returning a new metadata value.
    func metaSetting(_ key: String, _ value: Value) -> Metadata {
        var copy = self
        copy.metaSet(key, value)
        return copy
    }
}

/// A `u64` metadata value (canonicalized like the Rust/TS number model).
public func metaU64Value(_ v: UInt64) -> Value { .number(.canonical(unsigned: UInt128(v))) }
