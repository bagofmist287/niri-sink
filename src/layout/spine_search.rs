//! Read-only candidates from native allocation slots, never client buffers or render poses.
use std::collections::VecDeque;

use smithay::utils::{Logical, Rectangle};

use super::shrink_expand::WindowForm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpineSearchMode {
    Adjacent,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpineCandidate<I> {
    pub id: I,
    /// Shortest adjacency distance; zero for workspace-wide traversal.
    pub distance: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpineSearch<I> {
    pub mode: SpineSearchMode,
    pub origin: Option<I>,
    pub candidates: Vec<SpineCandidate<I>>,
}

#[derive(Debug)]
pub(crate) struct SpineSlot<I> {
    pub id: I,
    pub area: Rectangle<f64, Logical>,
    pub form: WindowForm,
    pub visible: bool,
}

fn overlaps(a: f64, a_size: f64, b: f64, b_size: f64) -> bool {
    (a + a_size).min(b + b_size) - a.max(b) > 1e-6
}

// Directions are deliberately stable: right, down, left, up. Each side scans
// the snapshot in native column/member order, including multiple neighbors.
fn neighbors<I>(slots: &[SpineSlot<I>], source: usize, gap: f64, epsilon: f64) -> Vec<usize> {
    let a = slots[source].area;
    let mut result = Vec::new();
    for side in 0..4 {
        for (index, slot) in slots.iter().enumerate() {
            if index == source || !slot.visible {
                continue;
            }
            let b = slot.area;
            let (distance, overlap) = match side {
                0 => (
                    b.loc.x - a.loc.x - a.size.w,
                    overlaps(a.loc.y, a.size.h, b.loc.y, b.size.h),
                ),
                1 => (
                    b.loc.y - a.loc.y - a.size.h,
                    overlaps(a.loc.x, a.size.w, b.loc.x, b.size.w),
                ),
                2 => (
                    a.loc.x - b.loc.x - b.size.w,
                    overlaps(a.loc.y, a.size.h, b.loc.y, b.size.h),
                ),
                _ => (
                    a.loc.y - b.loc.y - b.size.h,
                    overlaps(a.loc.x, a.size.w, b.loc.x, b.size.w),
                ),
            };
            if overlap && distance >= -epsilon && (distance - gap).abs() <= epsilon {
                result.push(index);
            }
        }
    }
    result
}

pub(crate) fn adjacent_candidates<I: PartialEq + Clone>(
    slots: &[SpineSlot<I>],
    origin: &I,
    gap: f64,
    epsilon: f64,
) -> Option<SpineSearch<I>> {
    let source = slots
        .iter()
        .position(|slot| &slot.id == origin && slot.visible && slot.form == WindowForm::Normal)?;
    let mut visited = vec![false; slots.len()];
    visited[source] = true;
    let mut queue = VecDeque::new();
    for index in neighbors(slots, source, gap, epsilon) {
        if let WindowForm::Spine(axis) = slots[index].form {
            if !visited[index] {
                visited[index] = true;
                queue.push_back((index, axis, 1));
            }
        }
    }
    let mut candidates = Vec::new();
    while let Some((index, axis, distance)) = queue.pop_front() {
        candidates.push(SpineCandidate {
            id: slots[index].id.clone(),
            distance,
        });
        for neighbor in neighbors(slots, index, gap, epsilon) {
            if !visited[neighbor] && slots[neighbor].form == WindowForm::Spine(axis) {
                visited[neighbor] = true;
                queue.push_back((neighbor, axis, distance + 1));
            }
        }
    }
    Some(SpineSearch {
        mode: SpineSearchMode::Adjacent,
        origin: Some(origin.clone()),
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::shrink_expand::ShrinkAxis;
    // Deliberately neither Hash nor Ord.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Id(usize);

    fn slot(id: usize, x: f64, y: f64, axis: Option<ShrinkAxis>) -> SpineSlot<Id> {
        SpineSlot {
            id: Id(id),
            area: Rectangle::new((x, y).into(), (100., 100.).into()),
            form: axis.map_or(WindowForm::Normal, WindowForm::Spine),
            visible: true,
        }
    }

    #[test]
    fn direct_sides_precede_breadth_first_same_axis_chains_and_normal_barriers() {
        use ShrinkAxis::{Horizontal as H, Vertical as V};
        let slots = vec![
            slot(0, 110., 110., None),
            slot(1, 220., 110., Some(H)),
            slot(2, 110., 220., Some(V)),
            slot(3, 0., 110., Some(H)),
            slot(4, 110., 0., Some(V)),
            slot(5, 330., 110., Some(H)),
            slot(6, 220., 220., Some(H)),
            slot(7, 110., 330., Some(H)),
            slot(8, 440., 110., None),
            slot(9, 550., 110., Some(H)),
            slot(10, 330., 220., Some(H)),
        ];
        let result = adjacent_candidates(&slots, &Id(0), 10., 1e-6).unwrap();
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|c| (c.id.0, c.distance))
                .collect::<Vec<_>>(),
            [(1, 1), (2, 1), (3, 1), (4, 1), (5, 2), (6, 2), (10, 3)]
        );
        assert_eq!(adjacent_candidates(&slots, &Id(0), 10., 1e-6), Some(result));
        assert!(adjacent_candidates(&slots, &Id(1), 10., 1e-6).is_none());
    }

    #[test]
    fn invisible_slots_and_corner_contact_do_not_create_edges() {
        let mut slots = vec![
            slot(0, 0., 0., None),
            slot(1, 110., 0., Some(ShrinkAxis::Horizontal)),
            slot(2, 220., 0., Some(ShrinkAxis::Horizontal)),
            slot(3, 110., 110., Some(ShrinkAxis::Horizontal)),
        ];
        slots[1].visible = false;
        assert!(adjacent_candidates(&slots, &Id(0), 10., 1e-6)
            .unwrap()
            .candidates
            .is_empty());
        slots[0].visible = false;
        assert!(adjacent_candidates(&slots, &Id(0), 10., 1e-6).is_none());
    }
}
