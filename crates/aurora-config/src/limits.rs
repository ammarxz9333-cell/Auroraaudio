//! Published bounds enforced by the configuration system.

/// Current production configuration schema version.
pub const CURRENT_SCHEMA_VERSION: u16 = 1;
/// Oldest schema version readable without migration.
pub const MINIMUM_READER_VERSION: u16 = 1;
/// Maximum UTF-8 bytes in an ordinary string field.
pub const MAX_STRING_BYTES: usize = 256;
/// Maximum UTF-8 bytes in a user description.
pub const MAX_DESCRIPTION_BYTES: usize = 1_024;
/// Maximum accepted serialized configuration or preset collection size.
pub const MAX_SERIALIZED_BYTES: usize = 256 * 1_024;
/// Maximum speakers in one layout.
pub const MAX_SPEAKERS: usize = 32;
/// Maximum channel routes in one configuration.
pub const MAX_ROUTES: usize = 64;
/// Maximum channel identities in either direction.
pub const MAX_CHANNELS: usize = 32;
/// Maximum presets in one collection.
pub const MAX_PRESETS: usize = 128;
/// Maximum tags on one preset.
pub const MAX_TAGS: usize = 16;
/// Maximum preset composition depth.
pub const MAX_COMPOSITION_DEPTH: usize = 8;
/// Maximum retained migration warnings.
pub const MAX_MIGRATION_DIAGNOSTICS: usize = 32;
/// Maximum allow-listed sample rates in a fallback policy.
pub const MAX_FALLBACK_SAMPLE_RATES: usize = 8;
