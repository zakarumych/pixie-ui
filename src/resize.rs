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
    layout::Arranged,
    math::{Pos, Rect, Size},
    style::{Attributes, WidgetSize},
    trigger::{
        invoke_drag, invoke_drag_end, invoke_drag_start, DragAction, DragEndAction,
        DragStartAction, NoAction,
    },
    ui::Ui,
    widget::Widget,
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
struct HitEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl HitEdges {
    fn any(&self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

fn hit_edges(rect: Rect, pos: Pos, border: i32) -> HitEdges {
    HitEdges {
        left: pos.x - rect.lt.x <= border,
        right: rect.rb.x - pos.x <= border,
        top: pos.y - rect.lt.y <= border,
        bottom: rect.rb.y - pos.y <= border,
    }
}

/// State committed once at drag-start by [`resize_on_drag_start`] and consumed by every
/// subsequent [`resize_on_drag`] call for that same gesture — see [`Resizable::anchor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DragAnchor {
    edges: HitEdges,
    position: Pos,
    size: Size,
    cursor: Pos,
}

/// Looks up the top-left corner `id`'s own `Attributes.position` is an offset from: its
/// parent's `Arranged.rect.lt`, or `ui.rect().lt` for a root widget (`Widget.parent == None`).
fn parent_top_left(world: &WorldLocal, id: EntityId) -> Option<Pos> {
    let widget_view = world.try_view_one::<&Widget>(id).ok()?;
    let parent = widget_view.get()?.parent;
    drop(widget_view);

    match parent {
        Some(parent) => {
            let arranged_view = world.try_view_one::<&Arranged>(parent).ok()?;
            arranged_view.get().map(|a| a.rect.lt)
        }
        None => world.get_resource::<Ui>().map(|ui| ui.rect().lt),
    }
}

/// Returns a [`DragStartAction`] that, paired with [`resize_on_drag`] and
/// [`resize_on_drag_end`], commits which edge/corner (if any) of the widget was grabbed — see
/// the module docs for the components a widget needs alongside it.
pub fn resize_on_drag_start() -> DragStartAction<NoAction> {
    invoke_drag_start(|world: &WorldLocal, id: EntityId, pos: Pos| {
        let Some(parent_lt) = parent_top_left(world, id) else {
            return;
        };

        let Ok(attrs_view) = world.try_view_one::<Cpy<Attributes>>(id) else {
            return;
        };
        let Some(attrs) = attrs_view.get() else {
            return;
        };
        drop(attrs_view);

        let Some(WidgetSize::Fixed(size)) = attrs.size else {
            return;
        };
        let Some(position) = attrs.position else {
            return;
        };

        let rect = Rect::from_pos_size(
            Pos {
                x: parent_lt.x + position.x,
                y: parent_lt.y + position.y,
            },
            size,
        );

        let Ok(mut resizable_view) = world.try_view_one::<&mut Resizable>(id) else {
            return;
        };
        let Some(resizable) = resizable_view.get_mut() else {
            return;
        };

        let edges = hit_edges(rect, pos, resizable.border);
        resizable.anchor = if edges.any() {
            Some(DragAnchor {
                edges,
                position,
                size,
                cursor: pos,
            })
        } else {
            None
        };
    })
}

/// Returns a [`DragAction`] that resizes the widget it's attached to, by growing/shrinking it
/// from the edge(s)/corner [`resize_on_drag_start`] committed at the start of this gesture — see
/// the module docs for the components a widget needs alongside it. No-op if no drag is currently
/// anchored (interior drag, or a widget missing [`resize_on_drag_start`]).
pub fn resize_on_drag() -> DragAction<NoAction> {
    invoke_drag(
        |world: &WorldLocal, id: EntityId, pos: Pos, _delta: crate::math::Vec| {
            let Ok(resizable_view) = world.try_view_one::<Cpy<Resizable>>(id) else {
                return;
            };
            let Some(resizable) = resizable_view.get() else {
                return;
            };
            drop(resizable_view);

            let Some(anchor) = resizable.anchor else {
                return;
            };

            // Total movement since the grab, not the per-event `delta`: every field below is
            // computed fresh from the anchor plus this absolute offset, so a fast/large single
            // `CursorMoved` can't desync anything the way accumulating `delta`-by-`delta` would.
            let total = pos - anchor.cursor;

            let max_w = resizable.max_size.map_or(i32::MAX, |s| s.w);
            let max_h = resizable.max_size.map_or(i32::MAX, |s| s.h);

            let mut size = anchor.size;
            let mut position = anchor.position;

            if anchor.edges.right {
                size.w = (anchor.size.w + total.x).clamp(resizable.min_size.w, max_w);
            }
            if anchor.edges.left {
                let new_w = (anchor.size.w - total.x).clamp(resizable.min_size.w, max_w);
                position.x = anchor.position.x + (anchor.size.w - new_w);
                size.w = new_w;
            }
            if anchor.edges.bottom {
                size.h = (anchor.size.h + total.y).clamp(resizable.min_size.h, max_h);
            }
            if anchor.edges.top {
                let new_h = (anchor.size.h - total.y).clamp(resizable.min_size.h, max_h);
                position.y = anchor.position.y + (anchor.size.h - new_h);
                size.h = new_h;
            }

            let Ok(mut view) = world.try_view_one::<&mut Attributes>(id) else {
                return;
            };
            let Some(attrs) = view.get_mut() else {
                return;
            };
            attrs.size = Some(WidgetSize::Fixed(size));
            attrs.position = Some(position);
        },
    )
}

/// Returns a [`DragEndAction`] that clears the anchor [`resize_on_drag_start`] committed, ending
/// the current resize gesture — see the module docs for the components a widget needs alongside
/// it.
pub fn resize_on_drag_end() -> DragEndAction<NoAction> {
    invoke_drag_end(|world: &WorldLocal, id: EntityId, _pos: Pos| {
        if let Ok(mut view) = world.try_view_one::<&mut Resizable>(id) {
            if let Some(resizable) = view.get_mut() {
                resizable.anchor = None;
            }
        }
    })
}
