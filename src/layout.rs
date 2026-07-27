use std::{cmp::Ordering, num::NonZero};

use edict::{
    component::Component,
    entity::{AliveEntity, EntityId, EntityLoc},
    query::Entities,
    view::View,
    world::World,
};
use smallvec::SmallVec;

use crate::{
    align::{Align, Align2},
    font::Font,
    margin::Margin,
    math::{Pos, Ratio, Rect, Size},
    style::{ResolvedAttributes, WidgetSize},
    text::Text,
    ui::Ui,
    widget::{Container, RootWidget, Widget},
};

/// Container layout kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentLayout {
    /// Widgets are stacked vertically, one after another.
    VerticalStack,

    /// Widgets are stacked horizontally, one after another.
    HorizontalStack,

    /// Widgets are arranged in a grid with the specified number of rows and columns.
    Grid { rows: u32, cols: u32 },
}

/// The computed minimum size of a widget, after measuring its content and children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct MinSize(pub Size);

/// The rect of a widget calculated by the layout system, after measuring and arranging its content and children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct Arranged {
    pub rect: Rect,
    pub layer: u32,
}

fn ensure_arranged_and_min_size(world: &mut World) {
    let world = world.local();
    let view = world
        .view::<(Entities, Option<&Arranged>, Option<&MinSize>)>()
        .with::<Widget>();

    for (e, arranged, min_size) in view {
        match (arranged, min_size) {
            (Some(_), Some(_)) => {
                // Both arranged and min_size are present
            }
            (Some(_), None) => {
                world.insert_defer(e, MinSize(Size::ZERO));
            }
            (None, Some(_)) => {
                world.insert_defer(
                    e,
                    Arranged {
                        rect: Rect::ZERO,
                        layer: 0,
                    },
                );
            }
            (None, None) => {
                world.insert_bundle_defer(
                    e,
                    (
                        Arranged {
                            rect: Rect::ZERO,
                            layer: 0,
                        },
                        MinSize(Size::ZERO),
                    ),
                );
            }
        }
    }

    world.run_deferred();
}

fn measure_tree(
    entity: impl AliveEntity,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, Option<&Container>, Option<&Text>)>,
    sizes: &mut View<&mut MinSize>,
) -> Size {
    let Some((attrs, container, text)) = view.get(entity) else {
        return Size::ZERO;
    };

    let mut content_size = Size::ZERO;

    if let Some(container) = container {
        let cl = attrs
            .0
            .content_layout
            .unwrap_or(ui.default_content_layout());

        match cl {
            ContentLayout::Grid { rows, cols } => {
                for row in 0..rows {
                    let mut row_size = Size::ZERO;

                    for col in 0..cols {
                        let child_index = (row * cols + col) as usize;
                        if let Some(&child) = container.children.get(child_index) {
                            if let Ok(child_entity) = world.lookup(child) {
                                let child_size = measure_tree(child_entity, world, ui, view, sizes);

                                row_size.w += child_size.w;
                                row_size.h = row_size.h.max(child_size.h);
                            }
                        }
                    }

                    content_size.w = content_size.w.max(row_size.w);
                    content_size.h += row_size.h;
                }
            }
            ContentLayout::HorizontalStack => {
                for &child in &container.children {
                    if let Ok(child_entity) = world.lookup(child) {
                        let child_size = measure_tree(child_entity, world, ui, view, sizes);

                        content_size.w += child_size.w;
                        content_size.h = content_size.h.max(child_size.h);
                    }
                }
            }
            ContentLayout::VerticalStack => {
                for &child in &container.children {
                    if let Ok(child_entity) = world.lookup(child) {
                        let child_size = measure_tree(child_entity, world, ui, view, sizes);

                        content_size.w = content_size.w.max(child_size.w);
                        content_size.h += child_size.h;
                    }
                }
            }
        }
    }

    if let Some(text) = text {
        if let Some(font) = ui.font(attrs.0.text_font.unwrap_or(ui.default_text_font())) {
            content_size += font.text_bbox(&*text.string).size();
        }
    }

    let final_size = match attrs.0.size {
        Some(WidgetSize::Fixed(size)) => size,
        _ => content_size,
    };

    if let Some(min_size) = sizes.get_mut(entity) {
        min_size.0 = final_size;
    };

    if attrs.0.position.is_some() {
        // Absolute-positioned widgets don't affect their parent's size.
        Size::ZERO
    } else {
        final_size
    }
}

/// Applies a [`Ratio`] to a pixel dimension, e.g. for [`WidgetSize::Relative`].
fn apply_ratio(value: i32, ratio: Ratio) -> i32 {
    (value as i64 * ratio.num as i64 / ratio.den.get() as i64) as i32
}

/// Arrange tree of widgets within the offered rect,
/// storing the result in the [`Arranged`] component of each widget
/// and returning the actual rect used for this widget if it is not absolutely positioned.
///
/// `parent_rect` is the resolved rect of this widget's immediate parent (or `ui.rect()` for a
/// root widget) — the basis for `WidgetSize::Relative`, which is deliberately independent of
/// `offer` (a flow child's `offer` may be a squeezed-down slot of the parent, but `Relative` is
/// defined as a ratio of the parent's own true size, capped by whatever it was actually offered).
///
/// `fallback_align` is the alignment to use when this widget's own `attrs.align` is `None`:
/// the caller's already-resolved `content_align` (or, for a root widget, `Ui`'s default).
fn arrange_tree(
    entity: impl AliveEntity,
    offer: Rect,
    parent_rect: Rect,
    fallback_align: Align2,
    layer: u32,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&Container>)>,
    arranged: &mut View<&mut Arranged>,
) -> Rect {
    let Some((attrs, min_size, container)) = view.get(entity) else {
        return Rect::ZERO;
    };

    let offer_w = (offer.rb.x - offer.lt.x).max(0);
    let offer_h = (offer.rb.y - offer.lt.y).max(0);
    let align = attrs.0.align.unwrap_or(fallback_align);
    let content_layout = attrs
        .0
        .content_layout
        .unwrap_or(ui.default_content_layout());
    let content_align = attrs.0.content_align.unwrap_or(ui.default_content_align());
    let inner_margin = attrs
        .0
        .inner_margin
        .unwrap_or_else(|| ui.default_inner_margin());

    // Flexible with at least one non-stretched axis needs a shrink-to-fit pass: this
    // widget's own size on that axis isn't known until we see how much its children
    // actually occupy, so `rect` starts out as just the raw `offer` (children get laid out
    // "as if stretched" — this is also what lets a Relative/Flexible descendant see real
    // available space instead of a starved guess). Every other variant already knows its
    // final size/position up front, so `rect` is final immediately and committed before
    // children are laid out — for those, `shrink_axes` is `None` and the second `match`
    // below is a no-op that just returns the same `rect`.
    let shrink_axes = match attrs.0.size {
        Some(WidgetSize::Flexible {
            stretches: (sx, sy),
        }) if !(sx && sy) => Some((sx, sy)),
        _ => None,
    };

    let rect = match shrink_axes {
        Some(_) => offer,
        None => {
            let size = resolve_size(attrs.0.size, min_size.0, offer_w, offer_h, parent_rect);
            let rect = resolve_rect(offer, size, attrs.0.position, align);
            if let Some(a) = arranged.get_mut(entity) {
                a.rect = rect;
                a.layer = layer;
            }
            rect
        }
    };

    let (occupied_w, occupied_h) = match container {
        Some(container) => arrange_children(
            rect,
            content_layout,
            content_align,
            inner_margin,
            layer,
            container,
            world,
            ui,
            view,
            arranged,
        ),
        None => (min_size.0.w, min_size.0.h),
    };

    let rect = match shrink_axes {
        None => rect,
        Some((sx, sy)) => {
            let size = Size {
                w: if sx {
                    offer_w
                } else {
                    occupied_w.max(min_size.0.w)
                },
                h: if sy {
                    offer_h
                } else {
                    occupied_h.max(min_size.0.h)
                },
            };
            let final_rect = resolve_rect(offer, size, attrs.0.position, align);

            if let Some(a) = arranged.get_mut(entity) {
                a.rect = final_rect;
                a.layer = layer;
            }

            // Children were arranged relative to `offer.lt`; if this widget's own final
            // origin ended up somewhere else (only possible via Center/End `align` or an
            // explicit `position`, since Start/no-position always yields the same origin as
            // `offer.lt`), shift the whole already-arranged subtree by the same delta — it's
            // a rigid translation, nothing needs recomputing.
            let delta = Pos {
                x: final_rect.lt.x - offer.lt.x,
                y: final_rect.lt.y - offer.lt.y,
            };
            if (delta.x != 0 || delta.y != 0)
                && let Some(container) = container
            {
                for &child in &container.children {
                    if let Ok(child_entity) = world.lookup(child) {
                        shift_subtree(child_entity, delta, world, view, arranged);
                    }
                }
            }

            final_rect
        }
    };

    if attrs.0.position.is_some() {
        Rect::ZERO
    } else {
        rect
    }
}

/// Resolves a widget's own size against the space it was offered. Only ever called for
/// variants whose size doesn't depend on children (`Fixed`, `Relative`, fully-stretched
/// `Flexible`, and no explicit size) — the partially-stretched `Flexible` shrink-to-fit case
/// is handled separately in [`arrange_tree`], after children are laid out.
fn resolve_size(
    size: Option<WidgetSize>,
    min_size: Size,
    offer_w: i32,
    offer_h: i32,
    parent_rect: Rect,
) -> Size {
    match size {
        // Fixed always uses its own stated size, then aligns within whatever it's offered —
        // same as a Flexible(false, false) would: it never stretches to fill.
        Some(WidgetSize::Fixed(size)) => size,
        // Relative stretches only up to a ratio of the *parent's* rect (not the possibly
        // squeezed-down `offer` a flow child gets from a distribute pass) — but never beyond
        // what it was actually offered, so a genuine squeeze (case B/C of `distribute_axis`)
        // still wins over the ratio ceiling.
        Some(WidgetSize::Relative(rw, rh)) => {
            let parent_w = (parent_rect.rb.x - parent_rect.lt.x).max(0);
            let parent_h = (parent_rect.rb.y - parent_rect.lt.y).max(0);
            Size {
                w: apply_ratio(parent_w, rw).max(0).min(offer_w),
                h: apply_ratio(parent_h, rh).max(0).min(offer_h),
            }
        }
        // Only reached here when both axes are stretched (the partially-stretched case is
        // handled by the shrink-to-fit path in `arrange_tree`) — fills the offer completely.
        Some(WidgetSize::Flexible {
            stretches: (sx, sy),
        }) => Size {
            w: if sx { offer_w.max(0) } else { min_size.w },
            h: if sy { offer_h.max(0) } else { min_size.h },
        },
        None => min_size,
    }
}

/// Resolves a widget's own rect from an already-known `size`: explicit `position` if set
/// (ignoring the offer's own position, but still relative to it — see `arrange_tree`'s
/// absolute-position doc comment), otherwise `size` aligned within `offer` (a no-op offset
/// when `offer` already fits `size` exactly, the common case for flow children since their
/// parent already carves out a tight offer).
fn resolve_rect(offer: Rect, size: Size, position: Option<Pos>, align: Align2) -> Rect {
    if let Some(pos) = position {
        // `pos` is an offset from the offer's own origin (i.e. from the parent container's
        // resolved top-left corner, or from `ui.rect().lt` for a root widget), not an
        // absolute/global position.
        let lt = Pos {
            x: offer.lt.x + pos.x,
            y: offer.lt.y + pos.y,
        };
        Rect {
            lt,
            rb: Pos {
                x: lt.x + size.w as i32,
                y: lt.y + size.h as i32,
            },
        }
    } else {
        align.in_rect(offer, size)
    }
}

/// Lays out `container`'s children within `rect`. `rect` is either this widget's own final
/// committed rect (the normal case), or — for a `Flexible` shrink-to-fit pass — a provisional
/// "as if fully stretched" rect (the raw `offer`) used only to discover how much space
/// children actually need on a non-stretched axis; see the shrink-to-fit branch of
/// [`arrange_tree`]. Returns the actual occupied extent of flow children, relative to
/// `rect.lt`: `(w, h)` — the normal caller ignores this, the shrink-to-fit caller uses it to
/// size this widget's own non-stretched axis/axes.
#[allow(clippy::too_many_arguments)]
fn arrange_children(
    rect: Rect,
    content_layout: ContentLayout,
    content_align: Align2,
    inner_margin: Margin,
    layer: u32,
    container: &Container,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&Container>)>,
    arranged: &mut View<&mut Arranged>,
) -> (i32, i32) {
    let mut occupied_w = 0i32;
    let mut occupied_h = 0i32;

    match content_layout {
        ContentLayout::Grid { rows, cols } => {
            let content_w = rect.rb.x - rect.lt.x;
            let content_h = rect.rb.y - rect.lt.y;

            // Group cells by row (independent horizontal sequences, per the existing
            // model: there are no shared/aligned column tracks across rows). Absolute
            // (explicitly positioned) cells are pulled out and arranged afterwards,
            // regardless of which row/col they'd have occupied.
            let mut grid_rows: SmallVec<[SmallVec<[FlowChild; 8]>; 8]> =
                SmallVec::with_capacity(rows as usize);
            let mut absolute: SmallVec<[EntityLoc; 8]> = SmallVec::new();

            for row in 0..rows {
                let mut row_cells = SmallVec::with_capacity(cols as usize);
                for col in 0..cols {
                    let idx = (row * cols + col) as usize;
                    if let Some(&child) = container.children.get(idx)
                        && let Some(classified) = classify_child(child, world, ui, view)
                    {
                        match classified {
                            ClassifiedChild::Flow(flow) => row_cells.push(flow),
                            ClassifiedChild::Absolute(entity) => absolute.push(entity),
                        }
                    }
                }
                grid_rows.push(row_cells);
            }

            // Row heights: each row acts as a pseudo-child of the vertical distribute
            // problem, aggregating its cells' mins/rels via max (mirroring how
            // `measure_tree` aggregates row height). Row margins are aggregated the same
            // way: a row's effective top/bottom outer margin is the max over its cells'.
            let row_axis: SmallVec<[(i32, Ratio); 8]> = grid_rows
                .iter()
                .map(|row_cells| {
                    let min = row_cells.iter().map(|c| c.min.h as i32).max().unwrap_or(0);
                    let rel = row_cells
                        .iter()
                        .map(|c| axis_rel_h(c.size))
                        .fold(Ratio::ZERO, ratio_max);
                    (min, rel)
                })
                .collect();
            let row_margin_top: SmallVec<[u8; 8]> = grid_rows
                .iter()
                .map(|row_cells| {
                    row_cells
                        .iter()
                        .map(|c| c.outer_margin.top)
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            let row_margin_bottom: SmallVec<[u8; 8]> = grid_rows
                .iter()
                .map(|row_cells| {
                    row_cells
                        .iter()
                        .map(|c| c.outer_margin.bottom)
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            let row_gaps_natural = natural_gaps(
                grid_rows.len(),
                inner_margin.top,
                inner_margin.bottom,
                |i| row_margin_top[i],
                |i| row_margin_bottom[i],
            );
            let (row_heights, row_gaps) = distribute_axis(&row_axis, &row_gaps_natural, content_h);

            let mut cursor_y = rect.lt.y;
            for (row_idx, row_cells) in grid_rows.iter().enumerate() {
                cursor_y += row_gaps[row_idx];
                let row_height = row_heights[row_idx];

                // Column widths within this row: independent per row, run the same
                // distribute algorithm over this row's own cells. Unlike row margins,
                // these are real individual cells, so each one's own outer margin is used
                // directly rather than aggregated via max.
                let col_axis: SmallVec<[(i32, Ratio); 8]> = row_cells
                    .iter()
                    .map(|c| (c.min.w as i32, axis_rel_w(c.size)))
                    .collect();
                let col_gaps_natural = natural_gaps(
                    row_cells.len(),
                    inner_margin.left,
                    inner_margin.right,
                    |i| row_cells[i].outer_margin.left,
                    |i| row_cells[i].outer_margin.right,
                );
                let (col_widths, col_gaps) =
                    distribute_axis(&col_axis, &col_gaps_natural, content_w);

                let mut cursor_x = rect.lt.x;
                for (col_idx, cell) in row_cells.iter().enumerate() {
                    cursor_x += col_gaps[col_idx];
                    let col_width = col_widths[col_idx];

                    let child_offer = Rect {
                        lt: Pos {
                            x: cursor_x,
                            y: cursor_y,
                        },
                        rb: Pos {
                            x: cursor_x + col_width,
                            y: cursor_y + row_height,
                        },
                    };
                    let child_rect = arrange_tree(
                        cell.entity,
                        child_offer,
                        rect,
                        content_align,
                        layer + 1,
                        world,
                        ui,
                        view,
                        arranged,
                    );
                    occupied_w = occupied_w.max(child_rect.rb.x - rect.lt.x);
                    occupied_h = occupied_h.max(child_rect.rb.y - rect.lt.y);
                    cursor_x += col_width;
                }

                cursor_y += row_height;
            }

            for entity in absolute {
                arrange_tree(
                    entity,
                    rect,
                    rect,
                    content_align,
                    layer + 1,
                    world,
                    ui,
                    view,
                    arranged,
                );
            }
        }
        ContentLayout::HorizontalStack => {
            let (flow, absolute) = classify_children(&container.children, world, ui, view);

            let axis: SmallVec<[(i32, Ratio); 8]> = flow
                .iter()
                .map(|c| (c.min.w as i32, axis_rel_w(c.size)))
                .collect();
            let gaps_natural = natural_gaps(
                flow.len(),
                inner_margin.left,
                inner_margin.right,
                |i| flow[i].outer_margin.left,
                |i| flow[i].outer_margin.right,
            );
            let (widths, gaps) = distribute_axis(&axis, &gaps_natural, rect.rb.x - rect.lt.x);

            let mut cursor_x = rect.lt.x;
            for (i, child) in flow.iter().enumerate() {
                cursor_x += gaps[i];
                let width = widths[i];

                // Cross-axis (height) inset: a simple per-child inset (not a distribute/
                // competition problem, since only one child occupies this span at a time)
                // by the larger of the container's own `inner_margin` and this child's own
                // `outer_margin` on each side.
                let top_inset = inner_margin.top.max(child.outer_margin.top) as i32;
                let bottom_inset = inner_margin.bottom.max(child.outer_margin.bottom) as i32;

                let child_offer = Rect {
                    lt: Pos {
                        x: cursor_x,
                        y: rect.lt.y + top_inset,
                    },
                    rb: Pos {
                        x: cursor_x + width,
                        y: rect.rb.y - bottom_inset,
                    },
                };
                let child_rect = arrange_tree(
                    child.entity,
                    child_offer,
                    rect,
                    content_align,
                    layer + 1,
                    world,
                    ui,
                    view,
                    arranged,
                );
                occupied_w = occupied_w.max(child_rect.rb.x - rect.lt.x);
                occupied_h = occupied_h.max(child_rect.rb.y - rect.lt.y);
                cursor_x += width;
            }

            for entity in absolute {
                arrange_tree(
                    entity,
                    rect,
                    rect,
                    content_align,
                    layer + 1,
                    world,
                    ui,
                    view,
                    arranged,
                );
            }
        }
        ContentLayout::VerticalStack => {
            let (flow, absolute) = classify_children(&container.children, world, ui, view);

            let axis: SmallVec<[(i32, Ratio); 8]> = flow
                .iter()
                .map(|c| (c.min.h as i32, axis_rel_h(c.size)))
                .collect();
            let gaps_natural = natural_gaps(
                flow.len(),
                inner_margin.top,
                inner_margin.bottom,
                |i| flow[i].outer_margin.top,
                |i| flow[i].outer_margin.bottom,
            );
            let (heights, gaps) = distribute_axis(&axis, &gaps_natural, rect.rb.y - rect.lt.y);

            let mut cursor_y = rect.lt.y;
            for (i, child) in flow.iter().enumerate() {
                cursor_y += gaps[i];
                let height = heights[i];

                // Cross-axis (width) inset: see the `HorizontalStack` branch above.
                let left_inset = inner_margin.left.max(child.outer_margin.left) as i32;
                let right_inset = inner_margin.right.max(child.outer_margin.right) as i32;

                let child_offer = Rect {
                    lt: Pos {
                        x: rect.lt.x + left_inset,
                        y: cursor_y,
                    },
                    rb: Pos {
                        x: rect.rb.x - right_inset,
                        y: cursor_y + height,
                    },
                };
                let child_rect = arrange_tree(
                    child.entity,
                    child_offer,
                    rect,
                    content_align,
                    layer + 1,
                    world,
                    ui,
                    view,
                    arranged,
                );
                occupied_w = occupied_w.max(child_rect.rb.x - rect.lt.x);
                occupied_h = occupied_h.max(child_rect.rb.y - rect.lt.y);
                cursor_y += height;
            }

            for entity in absolute {
                arrange_tree(
                    entity,
                    rect,
                    rect,
                    content_align,
                    layer + 1,
                    world,
                    ui,
                    view,
                    arranged,
                );
            }
        }
    }

    (occupied_w.max(0), occupied_h.max(0))
}

/// Translates `rect` by `delta`.
fn shift_rect(rect: Rect, delta: Pos) -> Rect {
    Rect {
        lt: Pos {
            x: rect.lt.x + delta.x,
            y: rect.lt.y + delta.y,
        },
        rb: Pos {
            x: rect.rb.x + delta.x,
            y: rect.rb.y + delta.y,
        },
    }
}

/// Rigidly translates `entity`'s already-computed `Arranged` rect, and recursively every
/// descendant's, by `delta`. Used by the `Flexible` shrink-to-fit branch of [`arrange_tree`]
/// when this widget's final alignment moves its origin relative to where its children were
/// already laid out: a pure translation, nothing about the arrangement itself is recomputed.
fn shift_subtree(
    entity: impl AliveEntity + Copy,
    delta: Pos,
    world: &World,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&Container>)>,
    arranged: &mut View<&mut Arranged>,
) {
    if let Some(a) = arranged.get_mut(entity) {
        a.rect = shift_rect(a.rect, delta);
    }

    if let Some((_, _, Some(container))) = view.get(entity) {
        let children = container.children.clone();
        for child in children {
            if let Ok(child_entity) = world.lookup(child) {
                shift_subtree(child_entity, delta, world, view, arranged);
            }
        }
    }
}

/// A flow child (i.e. `Attributes.position.is_none()`) collected while partitioning a
/// container's children, carrying just enough copied-out (small, `Copy`) data to run the
/// distribute algorithm without holding a live borrow of `view`.
#[derive(Clone, Copy)]
struct FlowChild<'w> {
    entity: EntityLoc<'w>,
    min: Size,
    size: Option<WidgetSize>,
    outer_margin: Margin,
}

/// Result of classifying one child entity as flow or absolute (see [`classify_child`]).
enum ClassifiedChild<'w> {
    Flow(FlowChild<'w>),
    Absolute(EntityLoc<'w>),
}

/// Looks up `child` and classifies it as a flow child (participates in the distribute
/// algorithm) or an absolute child (`Attributes.position.is_some()`, arranged afterwards
/// against the container's own resolved rect). Returns `None` if the entity no longer exists.
fn classify_child<'w>(
    child: EntityId,
    world: &'w World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&Container>)>,
) -> Option<ClassifiedChild<'w>> {
    let child_entity = world.lookup(child).ok()?;
    let (attrs, min, _) = view.get(child_entity)?;

    if attrs.0.position.is_some() {
        Some(ClassifiedChild::Absolute(child_entity))
    } else {
        Some(ClassifiedChild::Flow(FlowChild {
            entity: child_entity,
            min: min.0,
            size: attrs.0.size,
            outer_margin: attrs
                .0
                .outer_margin
                .unwrap_or_else(|| ui.default_outer_margin()),
        }))
    }
}

/// Partitions a flat list of children (as used by [`ContentLayout::HorizontalStack`] and
/// [`ContentLayout::VerticalStack`]) into flow children (in order) and absolute children.
fn classify_children<'w>(
    children: &[EntityId],
    world: &'w World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&Container>)>,
) -> (SmallVec<[FlowChild<'w>; 8]>, SmallVec<[EntityLoc<'w>; 8]>) {
    let mut flow: SmallVec<[FlowChild<'w>; 8]> = SmallVec::with_capacity(children.len());
    let mut absolute: SmallVec<[EntityLoc<'w>; 8]> = SmallVec::new();

    for &child in children {
        match classify_child(child, world, ui, view) {
            Some(ClassifiedChild::Flow(c)) => flow.push(c),
            Some(ClassifiedChild::Absolute(e)) => absolute.push(e),
            None => {}
        }
    }

    (flow, absolute)
}

/// The main-axis relative-size contribution of a child's own [`WidgetSize`], on the X axis,
/// for the purposes of the distribute algorithm (see [`distribute_axis`]): only `Relative(rw,
/// _)` contributes nonzero demand (`rw` literally). `Fixed`, `Flexible` (regardless of its
/// `stretches` flags), and `None` all contribute zero relative demand — they compete on
/// `min_i` alone. A `Flexible`-stretched child still ends up filling any leftover space handed
/// to it (case A of [`distribute_axis`] shares leftover evenly across all flow children, and
/// this child's own `arrange_tree` prologue then fills whatever offer it's given on a
/// stretched axis) — it just doesn't *claim* a proportional share the way `Relative` does.
fn axis_rel_w(size: Option<WidgetSize>) -> Ratio {
    match size {
        Some(WidgetSize::Relative(rw, _)) => rw,
        _ => Ratio::ZERO,
    }
}

/// The Y-axis counterpart of [`axis_rel_w`].
fn axis_rel_h(size: Option<WidgetSize>) -> Ratio {
    match size {
        Some(WidgetSize::Relative(_, rh)) => rh,
        _ => Ratio::ZERO,
    }
}

/// Compares two [`Ratio`]s via cross-multiplication (both denominators are always positive,
/// per `Ratio`'s invariants, so no sign flip is needed). Kept local to this module rather than
/// adding `Ord`/`PartialOrd` to `Ratio` itself in `math.rs`.
fn ratio_cmp(a: Ratio, b: Ratio) -> Ordering {
    let lhs = a.num as i64 * b.den.get() as i64;
    let rhs = b.num as i64 * a.den.get() as i64;
    lhs.cmp(&rhs)
}

fn ratio_le(a: Ratio, b: Ratio) -> bool {
    ratio_cmp(a, b) != Ordering::Greater
}

fn ratio_max(a: Ratio, b: Ratio) -> Ratio {
    if ratio_cmp(b, a) == Ordering::Greater {
        b
    } else {
        a
    }
}

/// Floors a [`Ratio`] to the nearest `i32` not greater than its value. `Ratio::den` is always
/// positive, so `div_euclid` gives a true floor (rounds toward negative infinity) here.
fn floor_ratio(r: Ratio) -> i32 {
    (r.num as i64).div_euclid(r.den.get() as i64) as i32
}

/// Builds the `natural_gaps` array (length `n + 1`) fed into [`distribute_axis`]: a leading gap
/// before the first of `n` children, one gap between each consecutive pair, and a trailing gap
/// after the last, each following the "max-collapse" rule (a shared boundary is the larger of
/// the two margins that meet there, never their sum) — `leading`/`trailing` are the container's
/// own `inner_margin` components on this axis (e.g. `.left`/`.right`, or `.top`/`.bottom`);
/// `before`/`after` extract child `i`'s own `outer_margin` components on the same axis (e.g.
/// `.left`/`.right` for a horizontal gap array, `.top`/`.bottom` for a vertical one).
fn natural_gaps(
    n: usize,
    leading: u8,
    trailing: u8,
    before: impl Fn(usize) -> u8,
    after: impl Fn(usize) -> u8,
) -> SmallVec<[i32; 8]> {
    let mut gaps = smallvec::smallvec![0i32; n + 1];
    if n == 0 {
        return gaps;
    }

    gaps[0] = (leading as i32).max(before(0) as i32);
    for (i, gap) in gaps.iter_mut().enumerate().take(n).skip(1) {
        *gap = (after(i - 1) as i32).max(before(i) as i32);
    }
    gaps[n] = (after(n - 1) as i32).max(trailing as i32);

    gaps
}

/// Distributes `leftover` (assumed to be in `0..values.len()` — the small integer remainder
/// left after flooring exact-`Ratio` contributions that summed to exactly `container_size`) by
/// adding one extra unit to the first `leftover` children in iteration order, guaranteeing the
/// returned sizes sum to exactly `values.len()`'s original sum plus `leftover`.
fn distribute_remainder(mut values: SmallVec<[i32; 8]>, leftover: i32) -> SmallVec<[i32; 8]> {
    let n = values.len() as i32;
    if n == 0 {
        return values;
    }

    let base = leftover.div_euclid(n);
    let rem = leftover.rem_euclid(n);

    for (i, v) in values.iter_mut().enumerate() {
        *v += base + if (i as i32) < rem { 1 } else { 0 };
    }

    values
}

/// Case C of [`distribute_axis`]: minimums alone don't fit (`min_sum > container_size`,
/// including the boundary `min_sum == container_size`). Shrinks every child proportionally by
/// the common factor `container_size / min_sum`, regardless of `rel_i`.
///
/// Note: this uniformly shrinks `Fixed`-size children below their stated fixed value too (their
/// `min_i` is treated identically to any other child's `min_i` here) — the literal reading of
/// "shrink by a common factor" with no stated exception. This is in tension with `Fixed` being
/// authoritative everywhere else in `arrange_tree`'s prologue: when this child's own
/// `arrange_tree` call resolves its size against the tight `child_offer` this produces, its
/// `Some(WidgetSize::Fixed(size)) => size` branch unconditionally re-asserts the full fixed
/// size, so in this specific extreme-overflow scenario a `Fixed` child may still visually
/// overflow/overlap its siblings, since nothing here (nor in the prologue) clips it. Left as-is
/// since fixing it is out of scope for this task.
fn distribute_case_c(
    children: &[(i32, Ratio)],
    container_size: i32,
    min_sum: i64,
) -> SmallVec<[i32; 8]> {
    let n = children.len();

    if min_sum <= 0 {
        // Degenerate: no flow children, or all have zero min size — nothing to scale.
        return smallvec::smallvec![0i32; n];
    }

    let min_sum = min_sum.clamp(1, i32::MAX as i64) as i32;
    let scale = Ratio::new(container_size, NonZero::new(min_sum).unwrap());

    let floored: SmallVec<[i32; 8]> = children
        .iter()
        .map(|&(min, _)| floor_ratio(Ratio::int(min) * scale))
        .collect();
    let floor_sum: i32 = floored.iter().sum();
    let leftover = container_size - floor_sum;

    distribute_remainder(floored, leftover)
}

/// Case B of [`distribute_axis`]: minimums all fit but relatives, if fully granted, would
/// overflow (`min_sum <= container_size < sum(preferred_i)`). Finds `X` in `[0, 1)` such that
/// `sum_i(max(want_i * X, min_i)) == container_size` via a water-filling sweep over ascending
/// thresholds `t_i = min_i / want_i`, then evaluates each child's exact contribution at that
/// `X`, floors, and distributes the integer remainder.
fn distribute_case_b(
    children: &[(i32, Ratio)],
    wants: &[Ratio],
    container_size: i32,
) -> SmallVec<[i32; 8]> {
    let target = Ratio::int(container_size);

    // Children with zero relative demand contribute a constant `min_i`, unaffected by `X`.
    // The rest ("relatives") each have a threshold `t_i = min_i / want_i` below/at which they
    // are still flat (`min_i`), and above which they grow (`want_i * X`).
    let mut flat_sum = Ratio::ZERO;
    let mut relatives: SmallVec<[(Ratio, Ratio, Ratio); 8]> = SmallVec::new(); // (min, want, threshold)

    for (&(min, _), &want) in children.iter().zip(wants) {
        if want == Ratio::ZERO {
            flat_sum = flat_sum + Ratio::int(min);
        } else {
            let threshold = Ratio::int(min) / want;
            relatives.push((Ratio::int(min), want, threshold));
        }
    }

    relatives.sort_by(|a, b| ratio_cmp(a.2, b.2));

    // Sweep ascending thresholds. Within each segment `[seg_start, next_threshold)`, the sum is
    // linear: `seg_start_value + slope * (X - seg_start)`, where `slope` is the sum of `want_i`
    // for children already past their threshold (growing) at the start of the segment.
    let mut seg_start = Ratio::ZERO;
    let mut seg_start_value = flat_sum + relatives.iter().fold(Ratio::ZERO, |acc, r| acc + r.0);
    let mut slope = Ratio::ZERO;

    let mut solved_x = None;

    for &(_min, want, threshold) in &relatives {
        let value_at_threshold = seg_start_value + slope * (threshold - seg_start);

        if ratio_le(seg_start_value, target) && ratio_le(target, value_at_threshold) {
            solved_x = Some(if slope == Ratio::ZERO {
                seg_start
            } else {
                seg_start + (target - seg_start_value) / slope
            });
            break;
        }

        seg_start = threshold;
        seg_start_value = value_at_threshold;
        slope = slope + want;
    }

    let x = solved_x.unwrap_or_else(|| {
        // Target lies beyond the last threshold: every relative child is growing.
        if slope == Ratio::ZERO {
            seg_start
        } else {
            seg_start + (target - seg_start_value) / slope
        }
    });

    let floored: SmallVec<[i32; 8]> = children
        .iter()
        .zip(wants)
        .map(|(&(min, _), &want)| {
            let contribution = if want == Ratio::ZERO {
                Ratio::int(min)
            } else {
                ratio_max(want * x, Ratio::int(min))
            };
            floor_ratio(contribution)
        })
        .collect();
    let floor_sum: i32 = floored.iter().sum();
    let leftover = container_size - floor_sum;

    distribute_remainder(floored, leftover)
}

/// Distributes `container_size` pixels of space along one axis across flow children, each
/// contributing a `min_i` (its `MinSize` on this axis) and `rel_i` (its relative-size ratio on
/// this axis, see [`axis_rel_w`]/[`axis_rel_h`]), following a CSS-flex-like scheme:
///
/// - **Case A** (free space): relatives and minimums all fit — leftover space is distributed
///   evenly (remainder biased to earlier children), *after* `natural_gaps` (the margin-derived
///   gap before/between/after children, see below) claim their share of the leftover first.
/// - **Case B** (relatives overflow, minimums fit): relatives are scaled down by a common
///   factor `X` (via a water-filling sweep) until they exactly fill `container_size`, never
///   going below each child's own `min_i`. Margins are dropped entirely (no free space to place
///   them in).
/// - **Case C** (minimums themselves overflow): every child is shrunk by the same proportional
///   factor `container_size / min_sum`, `rel_i` is ignored entirely. Margins are dropped
///   entirely here too.
///
/// `natural_gaps` (length `children.len() + 1`) is the desired leading/between/trailing gap
/// derived from margins (a leading gap before the first child, one gap between each consecutive
/// pair, and a trailing gap after the last). It's only honored in Case A:
/// - If the margins fit within the existing (margin-oblivious) leftover, children get their
///   usual Case A treatment against a leftover pool shrunk by the margin total, and the gaps
///   are used in full.
/// - If they don't (content fits but full margins would overflow), children get exactly their
///   no-margin preferred sizes (no growth into the leftover) and every gap is shrunk by the same
///   common factor, mirroring [`distribute_case_c`]'s shrink-by-common-factor pattern.
///
/// Returns `(sizes, gaps)`: in all three cases, `sizes` sums to exactly `container_size` minus
/// `gaps`'s sum, and `sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>() == container_size`
/// (clamped to `>= 0`) whenever Case A's gaps are actually used (Cases B/C return all-zero
/// gaps, so `sizes` alone sums to `container_size` there, same as before).
fn distribute_axis(
    children: &[(i32, Ratio)],
    natural_gaps: &[i32],
    container_size: i32,
) -> (SmallVec<[i32; 8]>, SmallVec<[i32; 8]>) {
    let n = children.len();
    if n == 0 {
        return (smallvec::smallvec![], natural_gaps.into());
    }

    let container_size = container_size.max(0);
    let min_sum: i64 = children.iter().map(|&(min, _)| min as i64).sum();

    if min_sum > container_size as i64 {
        let sizes = distribute_case_c(children, container_size, min_sum);
        return (sizes, smallvec::smallvec![0; n + 1]);
    }

    let wants: SmallVec<[Ratio; 8]> = children
        .iter()
        .map(|&(_, rel)| rel * Ratio::int(container_size))
        .collect();

    let preferred: SmallVec<[Ratio; 8]> = children
        .iter()
        .zip(&wants)
        .map(|(&(min, _), &want)| ratio_max(want, Ratio::int(min)))
        .collect();

    let preferred_sum = preferred.iter().fold(Ratio::ZERO, |acc, &p| acc + p);

    if ratio_le(preferred_sum, Ratio::int(container_size)) {
        // Case A.
        let floored: SmallVec<[i32; 8]> = preferred.iter().map(|&p| floor_ratio(p)).collect();
        let floor_sum: i32 = floored.iter().sum();
        let leftover = container_size - floor_sum;

        let margin_total: i32 = natural_gaps.iter().sum();

        if margin_total <= leftover {
            // Margins fit inside the no-margin leftover: children compete for whatever's
            // left after margins claim their share, gaps are used unshrunk.
            let sizes = distribute_remainder(floored, leftover - margin_total);
            return (sizes, natural_gaps.into());
        }

        // Margins don't fully fit even though content does: children get exactly their
        // no-margin preferred sizes (this fixup is a no-op in practice, since `floored`
        // already sums to `floor_sum`, but mirrors the same remainder-fixup mechanism used
        // everywhere else for clarity), and every gap is shrunk by the common factor
        // `leftover / margin_total` (guaranteed `margin_total > 0` here, since
        // `margin_total > leftover >= 0`).
        let sizes = distribute_remainder(floored, 0);

        let scale = Ratio::new(leftover, NonZero::new(margin_total).unwrap());
        let floored_gaps: SmallVec<[i32; 8]> = natural_gaps
            .iter()
            .map(|&g| floor_ratio(Ratio::int(g) * scale))
            .collect();
        let gap_floor_sum: i32 = floored_gaps.iter().sum();
        let gap_leftover = leftover - gap_floor_sum;
        let gaps = distribute_remainder(floored_gaps, gap_leftover);

        return (sizes, gaps);
    }

    // Case B.
    let sizes = distribute_case_b(children, &wants, container_size);
    (sizes, smallvec::smallvec![0; n + 1])
}

/// Resolves the [`Rect`] layout for every [`Widget`] entity, storing the result
/// in a [`ResolvedRect`] component.
///
/// # Precondition
///
/// Every `Widget` entity must already have a [`FinalAttributes`] component with
/// theme/fallback merging applied, i.e. this must run after `Style::resolve_attributes`.
pub fn layout_system(world: &mut World) {
    // Ensure that every widget has both an `Arranged` and a `MinSize` component.
    ensure_arranged_and_min_size(world);

    let Some(ui) = world.get_resource::<Ui>() else {
        return;
    };

    let roots = world.view::<Entities>().with::<RootWidget>();

    // Measure phase.
    // Calculates the minimum size of each widget.
    // For containers min size is calculated based on the min size of its children.
    {
        let mut view = world.view::<(&ResolvedAttributes, Option<&Container>, Option<&Text>)>();
        let view = view.lock().into();

        let mut sizes = world.view::<&mut MinSize>();
        let mut sizes = sizes.lock().into();

        // Step 2: resolve each root and its subtree.

        for root in roots.iter() {
            measure_tree(root, world, &ui, &view, &mut sizes);
        }
    }

    // Arrange phase.
    // Calculates the final rect of each widget based on its min size and the offered rect.
    {
        let mut view = world.view::<(&ResolvedAttributes, &MinSize, Option<&Container>)>();
        let view = view.lock().into();

        let mut arranged = world.view::<&mut Arranged>();
        let mut arranged = arranged.lock().into();

        // Step 2: resolve each root and its subtree.

        for root in roots.iter() {
            arrange_tree(
                root,
                ui.rect(),
                ui.rect(),
                ui.default_content_align(),
                0,
                world,
                &ui,
                &view,
                &mut arranged,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::style::Attributes;

    use super::*;

    fn r(num: i32, den: i32) -> Ratio {
        Ratio::new(num, NonZero::new(den).unwrap())
    }

    // --- Case A: plenty of room, leftover distributed evenly. ---

    #[test]
    fn distribute_case_a_equal_split() {
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let (sizes, gaps) = distribute_axis(&children, &[0, 0, 0], 30);
        assert_eq!(sizes[..], [15, 15]);
        assert_eq!(gaps[..], [0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 30);
    }

    #[test]
    fn distribute_case_a_remainder_biased_to_earlier_children() {
        let children = [(0, Ratio::ZERO), (0, Ratio::ZERO), (0, Ratio::ZERO)];
        let (sizes, gaps) = distribute_axis(&children, &[0, 0, 0, 0], 10);
        assert_eq!(sizes[..], [4, 3, 3]);
        assert_eq!(gaps[..], [0, 0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 10);
    }

    // --- Case A with margins: margins fit inside the no-margin leftover. ---

    #[test]
    fn distribute_case_a_margins_fit_use_full_gaps_and_max_collapse() {
        // Two children, both min=10, no relative demand: preferred sum = 20. Natural gaps
        // model a leading gap (container's own inner_margin vs. the first child's outer
        // margin, max-collapsed), a gap between the two children (max of their two outer
        // margins), and a trailing gap — totaling 9, comfortably inside the no-margin
        // leftover of 10 (container_size 30 - floor_sum 20).
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let natural_gaps = [2, 3, 4];
        let (sizes, gaps) = distribute_axis(&children, &natural_gaps, 30);

        // Gaps are used unshrunk (margins fully fit).
        assert_eq!(gaps[..], [2, 3, 4]);
        // Margins claim 9 of the 10 leftover pixels first; the remaining 1 is distributed
        // evenly (biased to the first child, same rule as the no-margin case).
        assert_eq!(sizes[..], [11, 10]);

        // Sums-exactly-right: sizes + gaps == container_size.
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 30);
    }

    #[test]
    fn distribute_case_a_margins_shrink_when_they_dont_fully_fit() {
        // Same two children (min=10 each, preferred sum 20, content fits inside
        // container_size 25 as before), but this time the natural gaps (5+5+5=15) exceed
        // the no-margin leftover (25-20=5): margins get shrunk by the common factor
        // 5/15 = 1/3, while children stay at their exact no-margin preferred size (10
        // each) — no growth into the leftover.
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let natural_gaps = [5, 5, 5];
        let (sizes, gaps) = distribute_axis(&children, &natural_gaps, 25);

        assert_eq!(sizes[..], [10, 10]);
        // Each gap floors to 1 (5 * 1/3 = 1.667), leaving 2 pixels of remainder biased to
        // the earlier gaps: [2, 2, 1].
        assert_eq!(gaps[..], [2, 2, 1]);

        // Sums-exactly-right: sizes + gaps == container_size, even in the shrunk case.
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 25);
    }

    // --- Case B: relatives overflow, minimums fit; solved via the water-filling sweep. ---

    #[test]
    fn distribute_case_b_single_relative_squeezed_by_fixed_sibling() {
        // child 0: no min, fully relative (rel = 1, i.e. "wants" the whole container).
        // child 1: fixed-like, min = 50, no relative demand.
        let children = [(10, Ratio::int(1)), (50, Ratio::ZERO)];
        let (sizes, gaps) = distribute_axis(&children, &[3, 4, 5], 80);
        assert_eq!(sizes[..], [30, 50]);
        // Margins are dropped entirely in Case B, regardless of what was requested.
        assert_eq!(gaps[..], [0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 80);
    }

    #[test]
    fn distribute_case_b_multiple_thresholds_in_sweep() {
        // child 0: no min, fully relative.
        // child 1: min = 20, half-relative.
        // child 2: fixed-like, min = 30, no relative demand.
        let children = [(0, Ratio::int(1)), (20, r(1, 2)), (30, Ratio::ZERO)];
        let (sizes, gaps) = distribute_axis(&children, &[0, 0, 0, 0], 100);
        assert_eq!(sizes[..], [47, 23, 30]);
        assert_eq!(gaps[..], [0, 0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 100);
    }

    // --- Case C: minimums themselves overflow; uniform proportional shrink. ---

    #[test]
    fn distribute_case_c_proportional_shrink() {
        let children = [(60, Ratio::ZERO), (60, Ratio::ZERO)];
        let (sizes, gaps) = distribute_axis(&children, &[7, 7, 7], 90);
        assert_eq!(sizes[..], [45, 45]);
        // Margins are dropped entirely in Case C too.
        assert_eq!(gaps[..], [0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 90);
    }

    #[test]
    fn distribute_case_c_degenerate_zero_min_sum() {
        let children: [(i32, Ratio); 0] = [];
        let (sizes, gaps) = distribute_axis(&children, &[0], 90);
        assert_eq!(sizes[..], []);
        assert_eq!(gaps[..], [0]);
    }

    // --- Integration: absolute-position offset fix and a small Grid case. ---

    fn spawn_widget(
        world: &mut World,
        parent: Option<EntityId>,
        attrs: Attributes,
        min: Size,
    ) -> EntityId {
        world
            .spawn((Widget { parent },))
            .insert(ResolvedAttributes(attrs))
            .unwrap()
            .insert(MinSize(min))
            .unwrap()
            .insert(Arranged {
                rect: Rect::ZERO,
                layer: 0,
            })
            .unwrap()
            .id()
    }

    fn spawn_container(
        world: &mut World,
        parent: Option<EntityId>,
        attrs: Attributes,
        min: Size,
        children: Vec<EntityId>,
    ) -> EntityId {
        let id = spawn_widget(world, parent, attrs, min);
        world
            .entity(id)
            .unwrap()
            .insert(Container { children })
            .unwrap();
        id
    }

    fn run_arrange(world: &World, root: EntityId, offer: Rect) {
        let ui = Ui::new();
        let mut view = world.view::<(&ResolvedAttributes, &MinSize, Option<&Container>)>();
        let view = view.lock().into();
        let mut arranged = world.view::<&mut Arranged>();
        let mut arranged = arranged.lock().into();

        let root = world.lookup(root).unwrap();
        arrange_tree(
            root,
            offer,
            offer,
            Align2::from(Align::Start),
            0,
            world,
            &ui,
            &view,
            &mut arranged,
        );
    }

    fn arranged_rect(world: &mut World, id: EntityId) -> Rect {
        world.get::<&Arranged>(id).unwrap().rect
    }

    #[test]
    fn absolute_position_is_offset_from_parent() {
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                position: Some(Pos { x: 5, y: 7 }),
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 10 })),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                // Fill the offer exactly, so the container's own resolved rect is
                // deterministic regardless of its (irrelevant, here) `MinSize`.
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 100, y: 200 },
            rb: Pos { x: 300, y: 400 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(root_rect, offer);

        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(
            child_rect,
            Rect {
                lt: Pos { x: 105, y: 207 },
                rb: Pos { x: 115, y: 217 },
            }
        );
    }

    #[test]
    fn grid_mixed_relative_and_fixed_cells() {
        let mut world = World::new();

        let cell0 = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 20, h: 15 })),
                ..Default::default()
            },
            Size { w: 20, h: 15 },
        );
        let cell1 = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Relative(r(1, 2), Ratio::int(1))),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::Grid { rows: 1, cols: 2 }),
                ..Default::default()
            },
            Size::ZERO,
            vec![cell0, cell1],
        );
        world.get::<&mut Widget>(cell0).unwrap().parent = Some(root);
        world.get::<&mut Widget>(cell1).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 100, y: 200 },
            rb: Pos { x: 200, y: 250 },
        };
        run_arrange(&world, root, offer);

        // Column widths: preferred = [max(0,20)=20, max(50,10)=50], sum = 70 <= 100 (case A),
        // leftover = 30 split evenly (both cells, including the `Fixed` one, per the literal
        // "distribute leftover evenly across all flow children" rule) -> widths [35, 65].
        let cell0_rect = arranged_rect(&mut world, cell0);
        assert_eq!(
            cell0_rect,
            Rect {
                lt: Pos { x: 100, y: 200 },
                rb: Pos { x: 120, y: 215 },
            }
        );

        // cell1's column slot is 65 wide (see above), but `Relative(1/2, 1)` caps its own size
        // at half of the *grid's* width (parent_rect, 100 -> 50), not half of its own slot —
        // so it resolves to 50x50 and aligns (Start) within the wider 65-wide slot.
        let cell1_rect = arranged_rect(&mut world, cell1);
        assert_eq!(
            cell1_rect,
            Rect {
                lt: Pos { x: 135, y: 200 },
                rb: Pos { x: 185, y: 250 },
            }
        );
    }

    #[test]
    fn flexible_shrink_to_fit_does_not_starve_children() {
        // A non-stretched Flexible container must lay its children out against the *real*
        // offer (not a bottom-up min_size, which is 0/unmeasured here in this hand-built
        // fixture) before shrinking to their occupied extent, so a Relative child inside it
        // still gets a real share of space rather than being squeezed to nothing.
        let mut world = World::new();

        let relative_child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Relative(r(1, 2), r(1, 4))),
                ..Default::default()
            },
            Size::ZERO,
        );
        let fixed_child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 20, h: 10 })),
                ..Default::default()
            },
            // MinSize mirrors what `measure_tree` would have computed for a Fixed widget
            // (its own fixed size) — this test hand-builds the `World` and skips the measure
            // phase, so it must supply that fixture directly.
            Size { w: 20, h: 10 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (false, false),
                }),
                content_layout: Some(ContentLayout::HorizontalStack),
                ..Default::default()
            },
            Size::ZERO,
            vec![relative_child, fixed_child],
        );
        world.get::<&mut Widget>(relative_child).unwrap().parent = Some(root);
        world.get::<&mut Widget>(fixed_child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 100, y: 40 },
        };
        run_arrange(&world, root, offer);

        // Column slots: preferred = [max(50,0)=50, max(0,20)=20], sum=70<=100 (case A),
        // leftover=30 split evenly -> slots [65, 35]. relative_child resolves to
        // min(65, 1/2*100)=50 (Start-aligned in its 65-wide slot); fixed_child resolves to
        // its own 20 regardless of slot (Start-aligned in its 35-wide slot, starting at
        // cursor 65). Occupied extent = fixed_child's rb.x = 65+20 = 85, so the container
        // shrinks to width 85 (not the full 100 offer, and not 0).
        let relative_rect = arranged_rect(&mut world, relative_child);
        assert_eq!(
            relative_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 50, y: 10 },
            }
        );

        let fixed_rect = arranged_rect(&mut world, fixed_child);
        assert_eq!(
            fixed_rect,
            Rect {
                lt: Pos { x: 65, y: 0 },
                rb: Pos { x: 85, y: 10 },
            }
        );

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 85, y: 10 },
            }
        );
    }

    #[test]
    fn flexible_shrink_to_fit_shifts_subtree_when_centered() {
        // A shrink-to-fit container that isn't Start-aligned ends up positioned somewhere
        // other than where its children were provisionally laid out (at the offer's origin)
        // — the whole already-arranged subtree must shift by the same delta, unchanged
        // otherwise.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 30, h: 20 })),
                ..Default::default()
            },
            Size::ZERO,
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (false, false),
                }),
                align: Some(Align2::from(Align::Center)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 200, y: 100 },
        };
        run_arrange(&world, root, offer);

        // Root shrinks to its single child's 30x20 and centers within the 200x100 offer.
        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 85, y: 40 },
                rb: Pos { x: 115, y: 60 },
            }
        );

        // The child, provisionally laid out at the offer's origin (0,0), must have been
        // shifted by the same (85, 40) delta to end up coincident with the shrunk root.
        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(child_rect, root_rect);
    }

    #[test]
    fn stack_cross_axis_inset_uses_max_of_inner_and_outer_margin() {
        // HorizontalStack: the cross axis is height. The container's own `inner_margin`
        // (top=5, bottom=3) loses to the child's larger `outer_margin` (top=20) on top, but
        // wins over the child's smaller one (bottom=2) on the bottom (max-collapse per side,
        // independently) — so the child ends up inset by 20 from the top and 3 from the
        // bottom, not flush against either edge.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 10 })),
                outer_margin: Some(Margin::new(20, 0, 2, 0)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::new(5, 0, 3, 0)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 100, y: 100 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(root_rect, offer);

        // Inset from the top by max(inner_margin.top=5, outer_margin.top=20)=20, from the
        // bottom by max(inner_margin.bottom=3, outer_margin.bottom=2)=3 — not flush against
        // either edge of the 100-tall container.
        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(
            child_rect,
            Rect {
                lt: Pos { x: 0, y: 20 },
                rb: Pos { x: 10, y: 30 },
            }
        );
    }

    #[test]
    fn grid_row_margin_aggregation_vs_cell_own_margin() {
        // A single Grid row with two cells whose outer margins differ: row heights go
        // through the margin-aware `distribute_axis` using the *row-aggregated* (max over
        // cells) top/bottom margin, while column widths within the row use each cell's own
        // margin directly (no aggregation — these are real individual cells).
        let mut world = World::new();

        // cell0: outer_margin top=6, left=1, bottom=7, right=2.
        let cell0 = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 10 })),
                outer_margin: Some(Margin::new(6, 1, 7, 2)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );
        // cell1: outer_margin top=9, left=8, bottom=1, right=0 — row-aggregated top/bottom
        // (max over both cells) end up 9 and 7 respectively, differing from cell1's own 9/1
        // and cell0's own 6/7.
        let cell1 = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 10 })),
                outer_margin: Some(Margin::new(9, 8, 1, 0)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::Grid { rows: 1, cols: 2 }),
                inner_margin: Some(Margin::new(2, 3, 4, 5)),
                ..Default::default()
            },
            Size::ZERO,
            vec![cell0, cell1],
        );
        world.get::<&mut Widget>(cell0).unwrap().parent = Some(root);
        world.get::<&mut Widget>(cell1).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 50, y: 40 },
        };
        run_arrange(&world, root, offer);

        // Row height: preferred = max(10,10) = 10 <= content_h (40) -> Case A. Row-aggregated
        // margins: top = max(cell0.top=6, cell1.top=9) = 9, bottom = max(cell0.bottom=7,
        // cell1.bottom=1) = 7. Leading gap = max(inner.top=2, 9) = 9, trailing gap =
        // max(7, inner.bottom=4) = 7; both comfortably fit inside the 30px no-margin
        // leftover (40 - 10), so row height grows to 10 + (30 - 16) = 24, and the row starts
        // at y = 0 + 9 = 9.
        //
        // Column widths within the row: preferred = [10, 10], sum 20 <= content_w (50) ->
        // Case A. Cell-own (not aggregated) margins: leading = max(inner.left=3,
        // cell0.left=1) = 3, between = max(cell0.right=2, cell1.left=8) = 8, trailing =
        // max(cell1.right=0, inner.right=5) = 5; all fit inside the 30px no-margin leftover
        // (50 - 20), so both columns grow to 10 + (30 - 16)/2 = 17.
        let cell0_rect = arranged_rect(&mut world, cell0);
        assert_eq!(
            cell0_rect,
            Rect {
                lt: Pos { x: 3, y: 9 },
                rb: Pos { x: 13, y: 19 },
            }
        );

        let cell1_rect = arranged_rect(&mut world, cell1);
        assert_eq!(
            cell1_rect,
            Rect {
                lt: Pos { x: 28, y: 9 },
                rb: Pos { x: 38, y: 19 },
            }
        );
    }
}
