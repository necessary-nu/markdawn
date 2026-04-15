//! Maps `CorrelationId`s to compartments in a `BlockSink`'s `UIStackView`.

use indexmap::IndexMap;

use crate::node::CorrelationId;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ElementLocation {
    /// The block lives inside a TextRun compartment. Use the compartment's
    /// `blocks` vec to find the actual char range.
    TextRun { compartment_index: usize },
    /// The block is its own custom UIView compartment.
    BlockView { compartment_index: usize },
}

pub(crate) type ElementMap = IndexMap<CorrelationId, ElementLocation>;

/// Adjust all compartment indices ≥ `from` by `delta`. Used after compartment
/// removal to keep the element map consistent with the stack.
pub(crate) fn shift_compartment_indices(map: &mut ElementMap, from: usize, delta: isize) {
    for loc in map.values_mut() {
        match loc {
            ElementLocation::TextRun { compartment_index }
            | ElementLocation::BlockView { compartment_index } => {
                if *compartment_index >= from {
                    *compartment_index = (*compartment_index as isize + delta) as usize;
                }
            }
        }
    }
}
