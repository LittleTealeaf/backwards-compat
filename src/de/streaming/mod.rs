mod adjacent;
mod keys;
mod map_de;
mod map_visit;
mod untagged;

#[cfg(test)]
mod tests;

pub use adjacent::{OptionForwardingDeserializer, drain_map, read_adjacent_content};
pub use keys::{AdjacentKey, AdjacentKeyResult, InternalKey, InternalKeyResult};
pub use map_de::{ChainedMapAccess, RemainingMapDeserializer};
pub use map_visit::{
    AdjacentMapDispatch, ExtField, ExtFieldDeserialize, IntTagMode, InternalMapDispatch, StrTagMode, TagMode,
    visit_adjacent_map, visit_adjacent_map_untagged, visit_internal_map, visit_internal_map_untagged,
};
pub use untagged::{
    UntaggedMultiDispatch, UntaggedMultiVisitor, UntaggedSingleDispatch, UntaggedSingleVisitor,
};

/// Caps pre-allocation from untrusted [`serde::de::SeqAccess::size_hint`] /
/// [`serde::de::MapAccess::size_hint`] values at 1 MiB worth of `T` elements so a crafted hint
/// cannot trigger an OOM panic.
#[inline]
#[must_use]
pub const fn cautious_size_hint<T>(hint: Option<usize>) -> usize {
    match hint {
        Some(h) => {
            let elem_size = core::mem::size_of::<T>();
            let max_elems = 1024 * 1024 / if elem_size > 0 { elem_size } else { 1 };
            if h < max_elems { h } else { max_elems }
        }
        None => 0,
    }
}
