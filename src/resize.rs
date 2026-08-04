//! A widget resized by dragging its own edges/corners, via [`crate::trigger::OnDrag`],
//! [`crate::trigger::OnDragStart`], and [`crate::trigger::OnDragEnd`].
//!
//! Requires the widget to be explicitly positioned and Fixed-sized
//! (`Attributes { position: Some(_), size: Some(WidgetSize::Fixed(_)), .. }`) — layout-flow-driven
//! size can't be locally overridden by a drag without fighting the layout algorithm every frame.
//! Attach [`Resizable`] for the size limits/grab-zone thickness, plus `SensesClicks` (needed for
//! press hit-testing) and `OnDragStart(resize_on_drag_start())`, `OnDrag(resize_on_drag())`,
//! `OnDragEnd(resize_on_drag_end())` to opt a widget into edge/corner dragging.

use edict::{component::Component, entity::EntityId, query::Cpy, world::WorldLocal};

use crate::{
    layout::{Arranged, Measure},
    math::{Pos, Rect, Size, Vec},
    style::{Attributes, WidgetPos, WidgetSize},
    trigger::{OnDrag, OnDragEnd, OnDragStart},
};

// NOTE: `crate::math::Vec` is deliberately *not* pulled into scope via `use` here (only
// referenced fully-qualified below, in `resize_on_drag`'s closure signature): the derived
// `Component` impl on `Resizable` below expands to code that references the unqualified
// (prelude) `std::vec::Vec`, and importing `math::Vec` unqualified into this module would shadow
// that prelude name and break the derive.

/// Attach alongside `Attributes { position: Some(_), size: Some(WidgetSize::Fixed(_)), .. }`,
/// `OnDragStart(resize_on_drag_start())`, `OnDrag(resize_on_drag())`, and
/// `OnDragEnd(resize_on_drag_end())` to let a widget be resized by dragging its own border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct Resizable {
    /// The smallest size this widget can be shrunk to.
    pub min_size: Size,

    /// The largest size this widget can be grown to, if any.
    pub max_size: Option<Size>,

    /// How close (in pixels) the cursor must be to an edge for it to count as grabbed, tested
    /// once at the start of a drag. A corner is grabbed wherever two adjacent edges' zones
    /// overlap.
    pub border: i32,

    /// Committed by [`resize_on_drag_start`] for the duration of one press-drag-release gesture:
    /// which edge(s) were grabbed, and the widget's own position/size and the cursor position at
    /// the moment of the grab. [`resize_on_drag`] computes every subsequent event's size purely
    /// as a function of this anchor plus the CURRENT cursor position — never incrementally from
    /// the previous event's size — so there's nothing to desync no matter how large a single
    /// `CursorMoved`'s movement is. Cleared back to `None` by [`resize_on_drag_end`].
    anchor: Option<DragAnchor>,
}

impl Resizable {
    pub const fn new(min_size: Size) -> Self {
        Resizable {
            min_size,
            max_size: None,
            border: 2,
            anchor: None,
        }
    }
}

/// Which edges of a rect a point was within [`Resizable::border`] pixels of, at the moment a
/// drag started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HitEdges {
    Left(i32),
    Top(i32),
    Right(i32),
    Bottom(i32),
    LeftTop(i32, i32),
    RightTop(i32, i32),
    LeftBottom(i32, i32),
    RightBottom(i32, i32),
}

/// Whether `pos` is within `border` pixels of any edge of `rect` — the same test
/// [`resize_on_drag_start`] uses to decide whether a drag grabs an edge/corner. Exposed for style
/// code that wants to highlight a [`Resizable`] widget's border on cursor proximity.
pub fn near_border(rect: Rect, pos: Pos, border: i32) -> bool {
    let left = pos.x - rect.lt.x;
    let top = pos.y - rect.lt.y;
    let right = rect.rb.x - pos.x;
    let bottom = rect.rb.y - pos.y;

    (left >= 0 && left <= border)
        || (top >= 0 && top <= border)
        || (right >= 0 && right <= border)
        || (bottom >= 0 && bottom <= border)
}

fn hit_edges(rect: Rect, pos: Pos, border: i32) -> Option<HitEdges> {
    let left = pos.x - rect.lt.x;
    let top = pos.y - rect.lt.y;
    let right = rect.rb.x - pos.x;
    let bottom = rect.rb.y - pos.y;

    match (
        (left >= 0 && left <= border),
        (top >= 0 && top <= border),
        (right >= 0 && right <= border),
        (bottom >= 0 && bottom <= border),
    ) {
        (true, true, false, false) => Some(HitEdges::LeftTop(left, top)),
        (false, true, true, false) => Some(HitEdges::RightTop(right, top)),
        (true, false, false, true) => Some(HitEdges::LeftBottom(left, bottom)),
        (false, false, true, true) => Some(HitEdges::RightBottom(right, bottom)),
        (true, false, false, false) => Some(HitEdges::Left(left)),
        (false, true, false, false) => Some(HitEdges::Top(top)),
        (false, false, true, false) => Some(HitEdges::Right(right)),
        (false, false, false, true) => Some(HitEdges::Bottom(bottom)),
        _ => None,
    }
}

/// State committed once at drag-start by [`resize_on_drag_start`] and consumed by every
/// subsequent [`resize_on_drag`] call for that same gesture — see [`Resizable::anchor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DragAnchor {
    edges: HitEdges,
}

fn widget_rect(world: &WorldLocal, id: EntityId) -> Option<Rect> {
    let view = world.try_view_one::<&Arranged>(id).ok()?;
    view.get().map(|a| a.rect)
}

fn widget_min_size(world: &WorldLocal, id: EntityId) -> Option<Size> {
    let view = world.try_view_one::<&Measure>(id).ok()?;
    view.get().map(|a| a.min)
}

fn is_widget_fixed_size(world: &WorldLocal, id: EntityId) -> bool {
    let Ok(view) = world.try_view_one::<&Attributes>(id) else {
        return false;
    };
    matches!(
        view.get().and_then(|a| a.size.as_ref()),
        Some(WidgetSize::Fixed(_))
    )
}

/// Returns a [`DragStartAction`] that, paired with [`resize_on_drag`] and
/// [`resize_on_drag_end`], commits which edge/corner (if any) of the widget was grabbed — see
/// the module docs for the components a widget needs alongside it.
pub fn resize_on_drag_start() -> OnDragStart {
    OnDragStart::invoke(|world: &WorldLocal, id: EntityId, pos: Pos, _delta: Vec| {
        if !is_widget_fixed_size(world, id) {
            return;
        }

        let Some(rect) = widget_rect(world, id) else {
            return;
        };

        let Ok(mut resizable_view) = world.try_view_one::<&mut Resizable>(id) else {
            return;
        };
        let Some(resizable) = resizable_view.get_mut() else {
            return;
        };

        resizable.anchor = if let Some(edges) = hit_edges(rect, pos, resizable.border) {
            Some(DragAnchor { edges })
        } else {
            None
        };
    })
}

/// Returns a [`DragAction`] that resizes the widget it's attached to, by growing/shrinking it
/// from the edge(s)/corner [`resize_on_drag_start`] committed at the start of this gesture — see
/// the module docs for the components a widget needs alongside it. No-op if no drag is currently
/// anchored (interior drag, or a widget missing [`resize_on_drag_start`]).
pub fn resize_on_drag() -> OnDrag {
    OnDrag::invoke(|world: &WorldLocal, id: EntityId, pos: Pos, _delta: Vec| {
        let Ok(mut view) =
            world.try_view_one::<(&mut Resizable, &mut Attributes, &Measure, &Arranged)>(id)
        else {
            return;
        };

        let Some((resizable, attributes, measure, arranged)) = view.get_mut() else {
            return;
        };
        let Some(WidgetSize::Fixed(fixed_size)) = attributes.size.as_mut() else {
            return;
        };
        let Some(anchor) = &mut resizable.anchor else {
            return;
        };

        let max_w = resizable.max_size.map_or(i32::MAX, |s| s.w);
        let max_h = resizable.max_size.map_or(i32::MAX, |s| s.h);

        let min_w = measure.min.w.max(resizable.min_size.w);
        let min_h = measure.min.h.max(resizable.min_size.h);

        match anchor.edges {
            HitEdges::Left(offset) => {
                let new_left = (pos.x - offset).clamp(
                    arranged.rect.rb.x.saturating_sub(max_w),
                    arranged.rect.rb.x.saturating_sub(min_w),
                );

                fixed_size.w = arranged.rect.rb.x.saturating_sub(new_left);
            }
            HitEdges::Top(offset) => {
                let new_top = (pos.y - offset).clamp(
                    arranged.rect.rb.y.saturating_sub(max_h),
                    arranged.rect.rb.y.saturating_sub(min_h),
                );

                fixed_size.h = arranged.rect.rb.y.saturating_sub(new_top);
            }
            HitEdges::Right(offset) => {
                let new_right = (pos.x + offset).clamp(
                    arranged.rect.lt.x.saturating_add(min_w),
                    arranged.rect.lt.x.saturating_add(max_w),
                );
                fixed_size.w = new_right.saturating_sub(arranged.rect.lt.x);
            }
            HitEdges::Bottom(offset) => {
                let new_bottom = (pos.y + offset).clamp(
                    arranged.rect.lt.y.saturating_add(min_h),
                    arranged.rect.lt.y.saturating_add(max_h),
                );
                fixed_size.h = new_bottom.saturating_sub(arranged.rect.lt.y);
            }
            HitEdges::LeftTop(offset_x, offset_y) => {
                let new_left = (pos.x - offset_x).clamp(
                    arranged.rect.rb.x.saturating_sub(max_w),
                    arranged.rect.rb.x.saturating_sub(min_w),
                );

                let new_top = (pos.y - offset_y).clamp(
                    arranged.rect.rb.y.saturating_sub(max_h),
                    arranged.rect.rb.y.saturating_sub(min_h),
                );

                fixed_size.w = arranged.rect.rb.x.saturating_sub(new_left);
                fixed_size.h = arranged.rect.rb.y.saturating_sub(new_top);
            }
            HitEdges::RightTop(offset_x, offset_y) => {
                let new_right = (pos.x + offset_x).clamp(
                    arranged.rect.lt.x.saturating_add(min_w),
                    arranged.rect.lt.x.saturating_add(max_w),
                );

                let new_top = (pos.y - offset_y).clamp(
                    arranged.rect.rb.y.saturating_sub(max_h),
                    arranged.rect.rb.y.saturating_sub(min_h),
                );

                fixed_size.w = new_right.saturating_sub(arranged.rect.lt.x);
                fixed_size.h = arranged.rect.rb.y.saturating_sub(new_top);
            }
            HitEdges::LeftBottom(offset_x, offset_y) => {
                let new_left = (pos.x - offset_x).clamp(
                    arranged.rect.rb.x.saturating_sub(max_w),
                    arranged.rect.rb.x.saturating_sub(min_w),
                );

                let new_bottom = (pos.y + offset_y).clamp(
                    arranged.rect.lt.y.saturating_add(min_h),
                    arranged.rect.lt.y.saturating_add(max_h),
                );

                fixed_size.w = arranged.rect.rb.x.saturating_sub(new_left);
                fixed_size.h = new_bottom.saturating_sub(arranged.rect.lt.y);
            }
            HitEdges::RightBottom(offset_x, offset_y) => {
                let new_right = (pos.x + offset_x).clamp(
                    arranged.rect.lt.x.saturating_add(min_w),
                    arranged.rect.lt.x.saturating_add(max_w),
                );

                let new_bottom = (pos.y + offset_y).clamp(
                    arranged.rect.lt.y.saturating_add(min_h),
                    arranged.rect.lt.y.saturating_add(max_h),
                );

                fixed_size.w = new_right.saturating_sub(arranged.rect.lt.x);
                fixed_size.h = new_bottom.saturating_sub(arranged.rect.lt.y);
            }
        };
    })
}

/// Returns a [`DragEndAction`] that clears the anchor [`resize_on_drag_start`] committed, ending
/// the current resize gesture — see the module docs for the components a widget needs alongside
/// it.
pub fn resize_on_drag_end() -> OnDragEnd {
    OnDragEnd::invoke(|world: &WorldLocal, id: EntityId, _pos: Pos| {
        if let Ok(mut view) = world.try_view_one::<&mut Resizable>(id) {
            if let Some(resizable) = view.get_mut() {
                resizable.anchor = None;
            }
        }
    })
}
