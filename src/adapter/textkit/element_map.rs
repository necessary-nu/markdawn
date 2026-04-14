use indexmap::IndexMap;
use objc2_foundation::NSRange;

use crate::node::CorrelationId;

#[derive(Debug, Clone)]
pub(crate) struct ElementRecord {
    pub range: NSRange,
}

pub(crate) type ElementMap = IndexMap<CorrelationId, ElementRecord>;

pub(crate) fn shift_ranges_after(map: &mut ElementMap, after: usize, delta: isize) {
    for record in map.values_mut() {
        if record.range.location >= after {
            record.range.location = (record.range.location as isize + delta) as usize;
        }
    }
}
