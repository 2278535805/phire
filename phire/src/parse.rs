mod extra;
pub use extra::parse_extra;

mod pec;
pub use pec::parse_pec;

mod pgr;
pub use pgr::parse_phigros;

mod rpe;
pub use rpe::{parse_rpe, RPE_HEIGHT, RPE_WIDTH, RPEChart};

pub(crate) fn process_lines(v: &mut [crate::core::JudgeLine]) {
    use crate::ext::NotNanExt;
    use ordered_float::NotNan;
    use rustc_hash::FxHashMap;

    let total_notes: usize = v.iter().map(|l| l.notes.len()).sum();
    let mut time_counts: FxHashMap<NotNan<f64>, u32> = FxHashMap::with_capacity_and_hasher(total_notes, Default::default());

    for note in v.iter().flat_map(|l| l.notes.iter()) {
        *time_counts.entry(note.time.not_nan()).or_insert(0) += 1;
    }

    for line in v.iter_mut() {
        for note in line.notes.iter_mut() {
            if let Some(&count) = time_counts.get(&note.time.not_nan()) {
                if count > 1 {
                    note.multiple_hint = true;
                }
            }
        }
    }
}

#[rustfmt::skip]
pub const RPE_TWEEN_MAP: [crate::core::TweenId; 30] = {
    use crate::core::{easing_from as e, TweenMajor::*, TweenMinor::*};
    [
        2, 2, // linear
        e(Sine, Out), e(Sine, In),
        e(Quad, Out), e(Quad, In),
        e(Sine, InOut), e(Quad, InOut),
        e(Cubic, Out), e(Cubic, In),
        e(Quart, Out), e(Quart, In),
        e(Cubic, InOut), e(Quart, InOut),
        e(Quint, Out), e(Quint, In),
        e(Expo, Out), e(Expo, In),
        e(Circ, Out), e(Circ, In),
        e(Back, Out), e(Back, In),
        e(Circ, InOut), e(Back, InOut),
        e(Elastic, Out), e(Elastic, In),
        e(Bounce, Out), e(Bounce, In),
        e(Bounce, InOut), e(Elastic, InOut),
    ]
};
