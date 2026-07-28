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
    align::Align2,
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

/// A widget's size if it — and its children's own margins — were fully honored, with no space
/// pressure. Computed bottom-up in `measure_tree`, alongside `MinSize`.
///
/// `folded`: this widget's full preferred size, INCLUDING this widget's own `inner_margin`. This
/// is what a *parent* reads when aggregating whether ITS OWN offer can satisfy every child's
/// demand in full — see `distribute_axis`'s `preferred_folded` parameter, which uses it to
/// compute a child's own margin-driven growth within the flat pool.
///
/// For a leaf (no `Container`), `folded == MinSize + inner_margin` (see the `Fixed`-size
/// exception below).
///
/// `WidgetSize::Fixed` overrides this field to the literal fixed value, ignoring content and
/// margin entirely — the same precedent `MinSize` already follows for `Fixed` (see
/// `measure_tree`'s `Some(WidgetSize::Fixed(size)) => size` arm). `Relative`/`Flexible` widgets
/// are offer-dependent and have no well-defined "preferred size" independent of an actual offer
/// (same reasoning `MinSize` already applies) — like `MinSize`, `PreferredSize` ignores the
/// `WidgetSize` variant for both `Relative` and `Flexible` and just reports content (+ margin),
/// regardless of what that variant would actually resolve to at arrange time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct PreferredSize {
    pub folded: Size,
}

/// The rect of a widget calculated by the layout system, after measuring and arranging its content and children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct Arranged {
    pub rect: Rect,
    pub layer: u32,
}

fn ensure_arranged_and_min_size(world: &mut World) {
    let world = world.local();
    let view = world
        .view::<(
            Entities,
            Option<&Arranged>,
            Option<&MinSize>,
            Option<&PreferredSize>,
        )>()
        .with::<Widget>();

    let default_arranged = || Arranged {
        rect: Rect::ZERO,
        layer: 0,
    };
    let default_preferred_size = || PreferredSize { folded: Size::ZERO };

    for (e, arranged, min_size, preferred_size) in view {
        match (arranged, min_size, preferred_size) {
            (Some(_), Some(_), Some(_)) => {
                // All three are already present.
            }
            (Some(_), Some(_), None) => {
                world.insert_defer(e, default_preferred_size());
            }
            (Some(_), None, Some(_)) => {
                world.insert_defer(e, MinSize(Size::ZERO));
            }
            (Some(_), None, None) => {
                world.insert_bundle_defer(e, (MinSize(Size::ZERO), default_preferred_size()));
            }
            (None, Some(_), Some(_)) => {
                world.insert_defer(e, default_arranged());
            }
            (None, Some(_), None) => {
                world.insert_bundle_defer(e, (default_arranged(), default_preferred_size()));
            }
            (None, None, Some(_)) => {
                world.insert_bundle_defer(e, (default_arranged(), MinSize(Size::ZERO)));
            }
            (None, None, None) => {
                world.insert_bundle_defer(
                    e,
                    (
                        default_arranged(),
                        MinSize(Size::ZERO),
                        default_preferred_size(),
                    ),
                );
            }
        }
    }

    world.run_deferred();
}

/// Looks up `entity`'s own `outer_margin` (falling back to `ui.default_outer_margin()`), used by
/// [`measure_tree`]'s container branch to feed [`natural_gaps`]/[`aggregate_cross_inset`] for its
/// bottom-up [`PreferredSize`] aggregation — the same margin resolution [`classify_child`] uses
/// at arrange time, just read directly from `view` instead of built into a [`FlowChild`].
fn child_outer_margin(
    entity: EntityId,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, Option<&Container>, Option<&Text>)>,
) -> Margin {
    world
        .lookup(entity)
        .ok()
        .and_then(|e| view.get(e))
        .map(|(attrs, _, _)| {
            attrs
                .0
                .outer_margin
                .unwrap_or_else(|| ui.default_outer_margin())
        })
        .unwrap_or_else(|| ui.default_outer_margin())
}

/// Recursively measures `entity` and its subtree, computing and storing both [`MinSize`] (the
/// unshrinkable minimum, margin-oblivious) and [`PreferredSize`] (the fully-honored preferred
/// size, margin-inclusive) for every widget in one bottom-up tree walk.
///
/// Returns `(min_size, preferred_size)`: the values used by this widget's *parent* to aggregate
/// its own sizes — both zeroed when `entity`'s own `Attributes.position` is set (an
/// absolutely-positioned widget doesn't affect its parent's size at all, on either component; see
/// each field's own doc comment for why `PreferredSize`'s stored component is also zeroed here,
/// unlike `MinSize`'s, which keeps its own real value stored even though the returned/aggregated
/// value is zero).
fn measure_tree(
    entity: impl AliveEntity,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, Option<&Container>, Option<&Text>)>,
    sizes: &mut View<&mut MinSize>,
    preferred_sizes: &mut View<&mut PreferredSize>,
) -> (Size, PreferredSize) {
    let zero_pref = PreferredSize { folded: Size::ZERO };

    let Some((attrs, container, text)) = view.get(entity) else {
        return (Size::ZERO, zero_pref);
    };

    let mut content_size = Size::ZERO;
    let mut container_pref: Option<PreferredSize> = None;

    if let Some(container) = container {
        let cl = attrs
            .0
            .content_layout
            .unwrap_or(ui.default_content_layout());
        let inner_margin = attrs
            .0
            .inner_margin
            .unwrap_or_else(|| ui.default_inner_margin());

        match cl {
            ContentLayout::Grid { rows, cols } => {
                let mut row_folded_w: SmallVec<[i32; 8]> = SmallVec::new();
                let mut row_heights: SmallVec<[i32; 8]> = SmallVec::new();
                let mut row_margin_top: SmallVec<[u8; 8]> = SmallVec::new();
                let mut row_margin_bottom: SmallVec<[u8; 8]> = SmallVec::new();

                for row in 0..rows {
                    let mut row_size = Size::ZERO;
                    let mut cell_folded_w: SmallVec<[i32; 8]> = SmallVec::new();
                    let mut cell_folded_h_max = 0i32;
                    let mut cell_outer_margins: SmallVec<[Margin; 8]> = SmallVec::new();

                    for col in 0..cols {
                        let child_index = (row * cols + col) as usize;
                        if let Some(&child) = container.children.get(child_index) {
                            if let Ok(child_entity) = world.lookup(child) {
                                let outer_margin = child_outer_margin(child, world, ui, view);
                                let (child_size, child_pref) =
                                    measure_tree(child_entity, world, ui, view, sizes, preferred_sizes);

                                row_size.w += child_size.w;
                                row_size.h = row_size.h.max(child_size.h);

                                cell_folded_w.push(child_pref.folded.w);
                                cell_folded_h_max = cell_folded_h_max.max(child_pref.folded.h);
                                cell_outer_margins.push(outer_margin);
                            }
                        }
                    }

                    content_size.w = content_size.w.max(row_size.w);
                    content_size.h += row_size.h;

                    let k = cell_folded_w.len();
                    let sum_w: i32 = cell_folded_w.iter().sum();
                    let gaps_folded = natural_gaps(
                        k,
                        inner_margin.left,
                        inner_margin.right,
                        |i| cell_outer_margins[i].left,
                        |i| cell_outer_margins[i].right,
                    );

                    row_folded_w.push(sum_w + gaps_folded.iter().sum::<i32>());
                    row_heights.push(cell_folded_h_max);
                    row_margin_top.push(
                        cell_outer_margins
                            .iter()
                            .map(|m| m.top)
                            .max()
                            .unwrap_or(0),
                    );
                    row_margin_bottom.push(
                        cell_outer_margins
                            .iter()
                            .map(|m| m.bottom)
                            .max()
                            .unwrap_or(0),
                    );
                }

                let folded_w = row_folded_w.iter().copied().max().unwrap_or(0);

                let n_rows = row_heights.len();
                let sum_row_heights: i32 = row_heights.iter().sum();
                let row_gaps_folded = natural_gaps(
                    n_rows,
                    inner_margin.top,
                    inner_margin.bottom,
                    |i| row_margin_top[i],
                    |i| row_margin_bottom[i],
                );

                container_pref = Some(PreferredSize {
                    folded: Size {
                        w: folded_w,
                        h: sum_row_heights + row_gaps_folded.iter().sum::<i32>(),
                    },
                });
            }
            ContentLayout::HorizontalStack => {
                let mut child_folded: SmallVec<[Size; 8]> = SmallVec::new();
                let mut outer_margins: SmallVec<[Margin; 8]> = SmallVec::new();

                for &child in &container.children {
                    if let Ok(child_entity) = world.lookup(child) {
                        let outer_margin = child_outer_margin(child, world, ui, view);
                        let (child_size, child_pref) =
                            measure_tree(child_entity, world, ui, view, sizes, preferred_sizes);

                        content_size.w += child_size.w;
                        content_size.h = content_size.h.max(child_size.h);

                        child_folded.push(child_pref.folded);
                        outer_margins.push(outer_margin);
                    }
                }

                let n = child_folded.len();
                let sum_w: i32 = child_folded.iter().map(|s| s.w).sum();
                let max_h = child_folded.iter().map(|s| s.h).max().unwrap_or(0);

                let gaps_folded = natural_gaps(
                    n,
                    inner_margin.left,
                    inner_margin.right,
                    |i| outer_margins[i].left,
                    |i| outer_margins[i].right,
                );

                let top_inset = aggregate_cross_inset(
                    outer_margins.iter().copied(),
                    inner_margin.top,
                    |m| m.top,
                );
                let bottom_inset = aggregate_cross_inset(
                    outer_margins.iter().copied(),
                    inner_margin.bottom,
                    |m| m.bottom,
                );

                container_pref = Some(PreferredSize {
                    folded: Size {
                        w: sum_w + gaps_folded.iter().sum::<i32>(),
                        h: max_h + top_inset + bottom_inset,
                    },
                });
            }
            ContentLayout::VerticalStack => {
                let mut child_folded: SmallVec<[Size; 8]> = SmallVec::new();
                let mut outer_margins: SmallVec<[Margin; 8]> = SmallVec::new();

                for &child in &container.children {
                    if let Ok(child_entity) = world.lookup(child) {
                        let outer_margin = child_outer_margin(child, world, ui, view);
                        let (child_size, child_pref) =
                            measure_tree(child_entity, world, ui, view, sizes, preferred_sizes);

                        content_size.w = content_size.w.max(child_size.w);
                        content_size.h += child_size.h;

                        child_folded.push(child_pref.folded);
                        outer_margins.push(outer_margin);
                    }
                }

                let n = child_folded.len();
                let sum_h: i32 = child_folded.iter().map(|s| s.h).sum();
                let max_w = child_folded.iter().map(|s| s.w).max().unwrap_or(0);

                let gaps_folded = natural_gaps(
                    n,
                    inner_margin.top,
                    inner_margin.bottom,
                    |i| outer_margins[i].top,
                    |i| outer_margins[i].bottom,
                );

                let left_inset = aggregate_cross_inset(
                    outer_margins.iter().copied(),
                    inner_margin.left,
                    |m| m.left,
                );
                let right_inset = aggregate_cross_inset(
                    outer_margins.iter().copied(),
                    inner_margin.right,
                    |m| m.right,
                );

                container_pref = Some(PreferredSize {
                    folded: Size {
                        w: max_w + left_inset + right_inset,
                        h: sum_h + gaps_folded.iter().sum::<i32>(),
                    },
                });
            }
        }
    }

    if let Some(text) = text {
        if let Some(font) = ui.font(attrs.0.font.unwrap_or(ui.default_font())) {
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

    let mut pref = container_pref.unwrap_or_else(|| {
        // Leaf (no `Container`): `folded` grows this widget's own `MinSize` (`final_size`,
        // which already applies the `Fixed` override below) by this widget's own
        // `inner_margin` — corrected back down to the bare `Fixed` value by the blanket
        // override below when applicable.
        let inner_margin = attrs
            .0
            .inner_margin
            .unwrap_or_else(|| ui.default_inner_margin());
        PreferredSize {
            folded: final_size + inner_margin.size(),
        }
    });

    // `Fixed` overrides this field to the literal fixed value, ignoring content and margin
    // entirely, for both leaves and containers alike (see `PreferredSize`'s doc comment).
    if let Some(WidgetSize::Fixed(size)) = attrs.0.size {
        pref = PreferredSize { folded: size };
    }

    // Absolute-positioned widgets don't affect their parent's size at all: unlike `MinSize`
    // (whose own stored component keeps its real value, only the value returned to the parent is
    // zeroed), `PreferredSize`'s own stored component is zeroed too — see the doc comment above.
    if attrs.0.position.is_some() {
        pref = zero_pref;
    }

    if let Some(preferred_size) = preferred_sizes.get_mut(entity) {
        *preferred_size = pref;
    };

    let min_size_ret = if attrs.0.position.is_some() {
        // Absolute-positioned widgets don't affect their parent's size.
        Size::ZERO
    } else {
        final_size
    };

    (min_size_ret, pref)
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
    view: &View<(&ResolvedAttributes, &MinSize, Option<&PreferredSize>, Option<&Container>)>,
    arranged: &mut View<&mut Arranged>,
) -> Rect {
    let Some((attrs, min_size, _preferred_size, container)) = view.get(entity) else {
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

    // `None` (no explicit size) is sugar for `Flexible { stretches: (false, false) }` — unify
    // by defaulting here so both go through the identical shrink-to-fit path below, with
    // identical semantics (including how `inner_margin` grows the widget).
    let widget_size = attrs.0.size.unwrap_or(WidgetSize::Flexible {
        stretches: (false, false),
    });

    // Flexible with at least one non-stretched axis needs a shrink-to-fit pass: this
    // widget's own size on that axis isn't known until we see how much its children
    // actually occupy, so `rect` starts out as just the raw `offer` (children get laid out
    // "as if stretched" — this is also what lets a Relative/Flexible descendant see real
    // available space instead of a starved guess). Every other variant already knows its
    // final size/position up front, so `rect` is final immediately and committed before
    // children are laid out — for those, `shrink_axes` is `None` and the second `match`
    // below is a no-op that just returns the same `rect`.
    let shrink_axes = match widget_size {
        WidgetSize::Flexible {
            stretches: (sx, sy),
        } if !(sx && sy) => Some((sx, sy)),
        _ => None,
    };

    // Whether this container itself stretches on each axis, for `distribute_axis`'s surplus
    // rule: `None` (both axes already fully committed before children are laid out — see
    // `shrink_axes`'s own doc comment) means surplus-to-gaps is harmless either way since
    // occupied extent isn't used to size this widget at all, so both count as stretching.
    let (stretches_w, stretches_h) = match shrink_axes {
        None => (true, true),
        Some((sx, sy)) => (sx, sy),
    };

    let rect = match shrink_axes {
        Some(_) => offer,
        None => {
            let size = resolve_size(widget_size, offer_w, offer_h, parent_rect);
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
            stretches_w,
            stretches_h,
            layer,
            container,
            world,
            ui,
            view,
            arranged,
        ),
        None => {
            let margin = inner_margin.size();
            (min_size.0.w + margin.w, min_size.0.h + margin.h)
        }
    };

    let rect = match shrink_axes {
        None => rect,
        Some((sx, sy)) => {
            let size = Size {
                w: if sx {
                    offer_w
                } else {
                    occupied_w.min(offer_w).max(min_size.0.w)
                },
                h: if sy {
                    offer_h
                } else {
                    occupied_h.min(offer_h).max(min_size.0.h)
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
/// variants whose size doesn't depend on children: `Fixed`, `Relative`, and `Flexible` —
/// which by this point is always `{ stretches: (true, true) }`, since any partial-stretch
/// `Flexible` (or `None`/no explicit size, defaulted into `Flexible { stretches: (false,
/// false) }`, see `arrange_tree`) is intercepted by `arrange_tree`'s `shrink_axes` check
/// before `resolve_size` is ever called, and handled separately after children are laid out.
fn resolve_size(size: WidgetSize, offer_w: i32, offer_h: i32, parent_rect: Rect) -> Size {
    match size {
        // Fixed always uses its own stated size, then aligns within whatever it's offered —
        // same as a Flexible(false, false) would: it never stretches to fill.
        WidgetSize::Fixed(size) => size,
        // Relative stretches only up to a ratio of the *parent's* rect (not the possibly
        // squeezed-down `offer` a flow child gets from a distribute pass) — but never beyond
        // what it was actually offered, so a genuine squeeze (`distribute_axis`'s shortage
        // regime, or its Case C) still wins over the ratio ceiling.
        WidgetSize::Relative(rw, rh) => {
            let parent_w = (parent_rect.rb.x - parent_rect.lt.x).max(0);
            let parent_h = (parent_rect.rb.y - parent_rect.lt.y).max(0);
            Size {
                w: apply_ratio(parent_w, rw).max(0).min(offer_w),
                h: apply_ratio(parent_h, rh).max(0).min(offer_h),
            }
        }
        // Always `(true, true)` here (see doc comment above) — fills the offer completely.
        WidgetSize::Flexible { .. } => Size {
            w: offer_w.max(0),
            h: offer_h.max(0),
        },
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
/// size this widget's own non-stretched axis/axes. The returned extent also includes the
/// container's own trailing `inner_margin`/gap on top of the children's own extent (whatever
/// space was actually available to grant it), matching the leading side, which was already
/// implicitly included via each child's offer.
#[allow(clippy::too_many_arguments)]
fn arrange_children(
    rect: Rect,
    content_layout: ContentLayout,
    content_align: Align2,
    inner_margin: Margin,
    // Whether the container being laid out (i.e. `rect`'s own owner) stretches on each axis —
    // fed straight into `distribute_axis`'s `container_stretches` parameter for whichever call
    // has that axis as its main axis, gating its surplus-to-gaps fallback (see `arrange_tree`,
    // which computes these from its own `shrink_axes`).
    stretches_w: bool,
    stretches_h: bool,
    layer: u32,
    container: &Container,
    world: &World,
    ui: &Ui,
    view: &View<(&ResolvedAttributes, &MinSize, Option<&PreferredSize>, Option<&Container>)>,
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
            // The row's own `preferred_folded`: the max over that row's cells' own
            // `preferred_folded.h` (mirroring how `row_axis`'s `min` component is already
            // max-aggregated, and how `measure_tree`'s Grid branch already max-aggregates
            // `folded` for rows). The row's own `stretches` flag is an OR across its cells — a
            // row counts as stretching if *any* cell in it wants to stretch vertically.
            let row_preferred_folded: SmallVec<[i32; 8]> = grid_rows
                .iter()
                .map(|row_cells| {
                    row_cells
                        .iter()
                        .map(|c| c.preferred_folded.h)
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            let row_stretches: SmallVec<[bool; 8]> = grid_rows
                .iter()
                .map(|row_cells| row_cells.iter().any(|c| axis_stretch_h(c.size)))
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
            let (row_heights, row_gaps) = distribute_axis(
                &row_axis,
                &row_preferred_folded,
                &row_stretches,
                &row_gaps_natural,
                content_h,
                stretches_h,
            );

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
                let col_preferred_folded: SmallVec<[i32; 8]> =
                    row_cells.iter().map(|c| c.preferred_folded.w).collect();
                let col_stretches: SmallVec<[bool; 8]> =
                    row_cells.iter().map(|c| axis_stretch_w(c.size)).collect();
                let col_gaps_natural = natural_gaps(
                    row_cells.len(),
                    inner_margin.left,
                    inner_margin.right,
                    |i| row_cells[i].outer_margin.left,
                    |i| row_cells[i].outer_margin.right,
                );
                let (col_widths, col_gaps) = distribute_axis(
                    &col_axis,
                    &col_preferred_folded,
                    &col_stretches,
                    &col_gaps_natural,
                    content_w,
                    stretches_w,
                );

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
                if !row_cells.is_empty() {
                    occupied_w = occupied_w.max(cursor_x - rect.lt.x + col_gaps[row_cells.len()]);
                }

                cursor_y += row_height;
            }
            if !grid_rows.is_empty() {
                occupied_h += row_gaps[grid_rows.len()];
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
            let preferred_folded: SmallVec<[i32; 8]> =
                flow.iter().map(|c| c.preferred_folded.w).collect();
            let stretches: SmallVec<[bool; 8]> =
                flow.iter().map(|c| axis_stretch_w(c.size)).collect();
            let gaps_natural = natural_gaps(
                flow.len(),
                inner_margin.left,
                inner_margin.right,
                |i| flow[i].outer_margin.left,
                |i| flow[i].outer_margin.right,
            );
            let (widths, gaps) = distribute_axis(
                &axis,
                &preferred_folded,
                &stretches,
                &gaps_natural,
                rect.rb.x - rect.lt.x,
                stretches_w,
            );

            // Cross-axis (height) inset: shared across every flow child so their cross-axis
            // edges align even when the margin has to shrink for the tightest one among them —
            // the per-child base (container's own `inner_margin` vs. that child's own
            // `outer_margin`, larger wins) is aggregated via `max` across all children first
            // (same pattern as Grid's row-margin aggregation above), then shrunk once against
            // whichever child needs the most room on this axis (see `shrink_cross_inset`) — a
            // child whose own content would have fit fine with the full margin still gets the
            // same shared, possibly-shrunk inset as its neediest sibling, not its own looser one.
            let top_inset_base = aggregate_cross_inset(
                flow.iter().map(|c| c.outer_margin),
                inner_margin.top,
                |m| m.top,
            );
            let bottom_inset_base = aggregate_cross_inset(
                flow.iter().map(|c| c.outer_margin),
                inner_margin.bottom,
                |m| m.bottom,
            );
            let content_min_h = flow.iter().map(|c| c.min.h as i32).max().unwrap_or(0);
            let (top_inset, bottom_inset) = shrink_cross_inset(
                top_inset_base,
                bottom_inset_base,
                rect.rb.y - rect.lt.y,
                content_min_h,
            );

            let mut cursor_x = rect.lt.x;
            for (i, child) in flow.iter().enumerate() {
                cursor_x += gaps[i];
                let width = widths[i];

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
                occupied_h = occupied_h.max(child_rect.rb.y - rect.lt.y + bottom_inset);
                cursor_x += width;
            }
            if !flow.is_empty() {
                occupied_w += gaps[flow.len()];
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
            let preferred_folded: SmallVec<[i32; 8]> =
                flow.iter().map(|c| c.preferred_folded.h).collect();
            let stretches: SmallVec<[bool; 8]> =
                flow.iter().map(|c| axis_stretch_h(c.size)).collect();
            let gaps_natural = natural_gaps(
                flow.len(),
                inner_margin.top,
                inner_margin.bottom,
                |i| flow[i].outer_margin.top,
                |i| flow[i].outer_margin.bottom,
            );
            let (heights, gaps) = distribute_axis(
                &axis,
                &preferred_folded,
                &stretches,
                &gaps_natural,
                rect.rb.y - rect.lt.y,
                stretches_h,
            );

            // Cross-axis (width) inset: see the `HorizontalStack` branch above — shared across
            // every flow child so their cross-axis edges align even when the margin has to
            // shrink for the tightest one among them.
            let left_inset_base = aggregate_cross_inset(
                flow.iter().map(|c| c.outer_margin),
                inner_margin.left,
                |m| m.left,
            );
            let right_inset_base = aggregate_cross_inset(
                flow.iter().map(|c| c.outer_margin),
                inner_margin.right,
                |m| m.right,
            );
            let content_min_w = flow.iter().map(|c| c.min.w as i32).max().unwrap_or(0);
            let (left_inset, right_inset) = shrink_cross_inset(
                left_inset_base,
                right_inset_base,
                rect.rb.x - rect.lt.x,
                content_min_w,
            );

            let mut cursor_y = rect.lt.y;
            for (i, child) in flow.iter().enumerate() {
                cursor_y += gaps[i];
                let height = heights[i];

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
                occupied_w = occupied_w.max(child_rect.rb.x - rect.lt.x + right_inset);
                occupied_h = occupied_h.max(child_rect.rb.y - rect.lt.y);
                cursor_y += height;
            }
            if !flow.is_empty() {
                occupied_h += gaps[flow.len()];
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
    view: &View<(&ResolvedAttributes, &MinSize, Option<&PreferredSize>, Option<&Container>)>,
    arranged: &mut View<&mut Arranged>,
) {
    if let Some(a) = arranged.get_mut(entity) {
        a.rect = shift_rect(a.rect, delta);
    }

    if let Some((_, _, _, Some(container))) = view.get(entity) {
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
    /// This child's own `PreferredSize.folded` — i.e. its full preferred size, including its own
    /// `inner_margin`. Fed into [`distribute_axis`]'s `preferred_folded` parameter to compute
    /// this child's own margin-driven growth (`margin_extra`) within the flat pool.
    preferred_folded: Size,
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
    view: &View<(&ResolvedAttributes, &MinSize, Option<&PreferredSize>, Option<&Container>)>,
) -> Option<ClassifiedChild<'w>> {
    let child_entity = world.lookup(child).ok()?;
    let (attrs, min, preferred_size, _) = view.get(child_entity)?;
    let preferred = preferred_size.copied().unwrap_or(PreferredSize { folded: min.0 });

    if attrs.0.position.is_some() {
        Some(ClassifiedChild::Absolute(child_entity))
    } else {
        Some(ClassifiedChild::Flow(FlowChild {
            entity: child_entity,
            min: min.0,
            preferred_folded: preferred.folded,
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
    view: &View<(&ResolvedAttributes, &MinSize, Option<&PreferredSize>, Option<&Container>)>,
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

/// Whether a child's own `WidgetSize` marks it as stretching on the X axis for the purposes of
/// surplus routing (see `distribute_axis`) — only `Flexible{stretches:(true,_)}` counts.
/// `Relative` has its own demand mechanism (folded into the pool via `size_extra`, see below),
/// and `None`/no explicit size is sugar for `Flexible{stretches:(false,false)}` (see
/// `arrange_tree`), so neither counts as stretching here.
fn axis_stretch_w(size: Option<WidgetSize>) -> bool {
    matches!(
        size,
        Some(WidgetSize::Flexible {
            stretches: (true, _)
        })
    )
}

/// The Y-axis counterpart of [`axis_stretch_w`].
fn axis_stretch_h(size: Option<WidgetSize>) -> bool {
    matches!(
        size,
        Some(WidgetSize::Flexible {
            stretches: (_, true)
        })
    )
}

/// Compares two [`Ratio`]s via cross-multiplication (both denominators are always positive,
/// per `Ratio`'s invariants, so no sign flip is needed). Kept local to this module rather than
/// adding `Ord`/`PartialOrd` to `Ratio` itself in `math.rs`.
fn ratio_cmp(a: Ratio, b: Ratio) -> Ordering {
    let lhs = a.num as i64 * b.den.get() as i64;
    let rhs = b.num as i64 * a.den.get() as i64;
    lhs.cmp(&rhs)
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

/// Aggregates a container's own `inner_margin` component against every flow child's own
/// matching `outer_margin` component (max-collapse per child, then the widest result wins
/// across all children) — the shared per-axis-side inset base used by both `arrange_children`
/// (for the real, possibly-shrunk inset) and `PreferredSize`'s bottom-up computation (for the
/// unshrunk preferred inset). Pass `inner_margin_side = 0` to get children's own `outer_margin`
/// contribution alone, with no container contribution — the same "strip this container's own
/// margin" trick `natural_gaps` uses when called with `leading = 0, trailing = 0`.
fn aggregate_cross_inset(
    outer_margins: impl Iterator<Item = Margin>,
    inner_margin_side: u8,
    side: impl Fn(Margin) -> u8,
) -> i32 {
    outer_margins
        .map(|m| inner_margin_side.max(side(m)))
        .max()
        .unwrap_or(0) as i32
}

/// Shrinks a leading/trailing cross-axis inset pair (a container's own `inner_margin` collapsed
/// against a child's `outer_margin`, see the `HorizontalStack`/`VerticalStack` branches of
/// [`arrange_children`]) so together they never claim more than the leftover space after
/// reserving `content_min` — the child's own minimum size on this same cross axis. Mirrors
/// [`distribute_axis`]'s Case A/B/C on the main axis: if the leftover comfortably fits the full
/// inset pair, both pass through unchanged (this container's Case A); if content fits but the
/// full inset doesn't, the pair is scaled down by the common factor `leftover / (leading +
/// trailing)` to consume exactly the leftover, mirroring [`distribute_case_c`]'s
/// shrink-by-common-factor pattern (this container's Case B); if `content_min` itself already
/// consumes all of `available` (no leftover at all), the inset is dropped entirely — both sides
/// become `0` — so content always gets priority over margin (this container's Case C, matching
/// how main-axis margins are "dropped entirely" once minimums themselves overflow).
pub(crate) fn shrink_cross_inset(leading: i32, trailing: i32, available: i32, content_min: i32) -> (i32, i32) {
    let sum = leading + trailing;
    if sum <= 0 {
        return (leading, trailing);
    }

    let leftover = (available - content_min).max(0);
    if sum <= leftover {
        return (leading, trailing);
    }
    if leftover <= 0 {
        return (0, 0);
    }

    let scale = Ratio::new(leftover, NonZero::new(sum).unwrap());
    let leading = floor_ratio(Ratio::int(leading) * scale);
    let trailing = leftover - leading;

    (leading, trailing)
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
/// Saturating water-fill: given `n` non-negative caps and a `budget <= sum(caps)`, finds `X`
/// such that `sum(min(cap_i, X)) == budget` — each cap either passes through unchanged (if
/// already `<= X`) or gets clamped down to `X`. The smallest caps are preserved longest, the
/// largest give way first — "give up the largest values first" (no per-item weighting; this is
/// deliberately a leveling algorithm, not a proportional one — see [`distribute_axis`]'s doc
/// comment for why). Implemented as a sort + running-count sweep: repeatedly grant the
/// next-smallest cap in full whenever an even split of the remaining budget across the remaining
/// (not-yet-granted) caps would exceed it, until the remaining caps all get the same final `X`.
fn water_fill_shrink(caps: &[i32], budget: i32) -> SmallVec<[i32; 16]> {
    let n = caps.len();
    if n == 0 {
        return SmallVec::new();
    }
    let budget = budget.max(0);

    let mut order: SmallVec<[usize; 16]> = (0..n).collect();
    order.sort_by_key(|&i| caps[i]);

    let mut remaining_budget = budget;
    let mut remaining_count = n as i32;
    let mut floor_idx = 0usize;
    while floor_idx < n {
        let cap = caps[order[floor_idx]];
        if cap as i64 * remaining_count as i64 > remaining_budget as i64 {
            break;
        }
        remaining_budget -= cap;
        remaining_count -= 1;
        floor_idx += 1;
    }

    let mut result: SmallVec<[i32; 16]> = smallvec::smallvec![0i32; n];
    for &i in &order[..floor_idx] {
        result[i] = caps[i];
    }
    if remaining_count > 0 {
        let base = remaining_budget / remaining_count;
        let rem = remaining_budget % remaining_count;
        for (k, &i) in order[floor_idx..].iter().enumerate() {
            result[i] = base + if (k as i32) < rem { 1 } else { 0 };
        }
    }

    result
}

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

/// Distributes `container_size` pixels of space along one axis across flow children, each
/// contributing a `min_i` (its `MinSize` on this axis) and `rel_i` (its relative-size ratio on
/// this axis, see [`axis_rel_w`]/[`axis_rel_h`]), following a two-regime scheme:
///
/// - **Case C** (minimums themselves overflow, `min_sum > container_size`): every child is
///   shrunk by the same proportional factor `container_size / min_sum`, `rel_i` is ignored
///   entirely. Margins/gaps are dropped entirely too. See [`distribute_case_c`].
/// - **Everything else** (`min_sum <= container_size`): only bare `min_i` is an unconditional
///   floor from here on. Everything beyond that — each child's `Relative`-want (`size_extra`),
///   each child's own margin-driven growth (`margin_extra`, `PreferredSize.folded` minus the
///   want-adjusted floor), and the container's own margin/sibling-outer-margin gaps
///   (`natural_gaps`) — becomes one flat pool. If the pool fits within `available_extra`
///   (`container_size - min_sum`) in full, it's a **surplus**: every gap and every child's
///   want/margin-extra is granted in full, and the true leftover routes to `Flexible{stretches}`
///   children first (if any on this axis, regardless of `container_stretches` — a genuinely
///   stretching child still grows even inside a shrink-to-fit container); otherwise, *if the
///   container itself stretches on this axis* (`container_stretches`), it grows the gaps —
///   non-stretchy content never grows past what it actually wants. If neither applies (no
///   stretching children AND the container itself is shrink-to-fit on this axis), the surplus
///   goes unconsumed entirely: `sizes`/`gaps` stay at their fully-granted, non-surplus values, so
///   the caller's occupied extent ends up less than `container_size` — exactly the shrink-to-fit
///   behavior a non-stretching container is supposed to have. If the pool doesn't fit, it's a
///   **shortage**: the largest demands (whichever they are — a gap, a `Relative` want, or margin)
///   shrink first, via [`water_fill_shrink`] — a leveling water-fill, *not* a proportional one:
///   two competing `Relative` siblings level toward equal size under shortage rather than
///   preserving their ratio (weighted/proportional shrink for competing `Relative` siblings is
///   intentionally deferred to a future version — this is a real, acknowledged behavior change,
///   not an oversight).
///
/// `natural_gaps` (length `children.len() + 1`) is the desired leading/between/trailing gap
/// derived from margins (a leading gap before the first child, one gap between each consecutive
/// pair, and a trailing gap after the last) — it's part of the same flat pool described above in
/// the non-Case-C regime, and dropped entirely (all zero) in Case C.
///
/// `preferred_folded[i]` is child `i`'s own `PreferredSize.folded` on this axis, and
/// `stretches[i]` is whether child `i` — a *child* of the container being distributed — is a
/// stretching `Flexible` on this axis (see [`axis_stretch_w`]/[`axis_stretch_h`]) — both only
/// consulted outside Case C. `container_stretches` is a different, container-level flag: whether
/// the container itself (i.e. the widget whose `rect` this axis is being carved out of) stretches
/// on this axis — see the surplus-regime description above for how the two interact.
///
/// Returns `(sizes, gaps)`: `sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>() ==
/// container_size` (clamped to `>= 0`) in the non-Case-C regime; Case C returns all-zero gaps,
/// so `sizes` alone sums to `container_size` there.
fn distribute_axis(
    children: &[(i32, Ratio)],
    preferred_folded: &[i32],
    stretches: &[bool],
    natural_gaps: &[i32],
    container_size: i32,
    container_stretches: bool,
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

    // Only bare `min_i` is an unconditional floor from here on. Everything above `min_sum` —
    // each child's `Relative`-want (`size_extra`, capped by whichever is larger of its ratio
    // target or nothing beyond min — `Fixed`/`Flexible`/`None` all naturally contribute `0`
    // here since their `rel_i` is `Ratio::ZERO`), each child's own margin-driven growth
    // (`margin_extra`, `PreferredSize.folded` minus the want-adjusted floor), and the
    // container's own margin/sibling-outer-margin gaps — becomes one flat pool, water-filled
    // together: the largest demand shrinks first (down toward the smallest), no weighting.
    let min_sum_i32 = min_sum as i32;
    let available_extra = container_size - min_sum_i32;

    let size_extra: SmallVec<[i32; 8]> = children
        .iter()
        .map(|&(min, rel)| {
            let want = floor_ratio(rel * Ratio::int(container_size));
            (want - min).max(0)
        })
        .collect();
    // Baseline against the want-adjusted floor (`min_i + size_extra_i`), not bare `min_i` —
    // `size_extra` and `margin_extra` are alternative *and* additive claims: a child's
    // `PreferredSize.folded` already represents "min + this child's own margin", independent
    // of whatever its `Relative` ratio wants, so once its want is (possibly partially) granted
    // the margin only needs to cover whatever's still missing above that adjusted floor, not
    // the whole gap from bare `min_i` again (that would double-count).
    let margin_extra: SmallVec<[i32; 8]> = children
        .iter()
        .zip(&size_extra)
        .zip(preferred_folded)
        .map(|((&(min, _), &se), &pf)| (pf - min - se).max(0))
        .collect();

    let n_gaps = natural_gaps.len();
    let mut pool: SmallVec<[i32; 16]> = SmallVec::with_capacity(n_gaps + 2 * n);
    pool.extend_from_slice(natural_gaps);
    pool.extend(size_extra.iter().copied());
    pool.extend(margin_extra.iter().copied());
    let pool_sum: i32 = pool.iter().sum();

    if available_extra >= pool_sum {
        // Surplus: every gap and every child's want/margin-extra granted in full. The true
        // leftover routes to `Flexible`-stretch children first (if any on this axis), else
        // grows the gaps — non-stretchy content never grows past what it actually wants.
        let surplus = available_extra - pool_sum;

        let mut sizes: SmallVec<[i32; 8]> = children
            .iter()
            .zip(&size_extra)
            .zip(&margin_extra)
            .map(|((&(min, _), &se), &me)| min + se + me)
            .collect();
        let mut gaps: SmallVec<[i32; 8]> = natural_gaps.into();

        let stretch_indices: SmallVec<[usize; 8]> =
            (0..n).filter(|&i| stretches[i]).collect();
        if !stretch_indices.is_empty() {
            let m = stretch_indices.len() as i32;
            let base = surplus / m;
            let rem = surplus % m;
            for (k, &i) in stretch_indices.iter().enumerate() {
                sizes[i] += base + if (k as i32) < rem { 1 } else { 0 };
            }
        } else if container_stretches && n_gaps > 0 {
            let m = n_gaps as i32;
            let base = surplus / m;
            let rem = surplus % m;
            for (k, g) in gaps.iter_mut().enumerate() {
                *g += base + if (k as i32) < rem { 1 } else { 0 };
            }
        }
        // else: no stretching children AND the container itself is shrink-to-fit on this axis
        // — surplus goes unconsumed. `sizes`/`gaps` stay at their fully-granted (non-surplus)
        // values, so the caller's occupied extent ends up less than `container_size`.

        return (sizes, gaps);
    }

    // Shortage: the combined pool doesn't fully fit — the largest demands (whichever they
    // are: a gap, a `Relative` want, or margin) shrink first, via `water_fill_shrink`.
    let granted = water_fill_shrink(&pool, available_extra);
    let granted_gaps = &granted[..n_gaps];
    let granted_size_extra = &granted[n_gaps..n_gaps + n];
    let granted_margin_extra = &granted[n_gaps + n..];

    let sizes: SmallVec<[i32; 8]> = children
        .iter()
        .zip(granted_size_extra)
        .zip(granted_margin_extra)
        .map(|((&(min, _), &se), &me)| min + se + me)
        .collect();
    let gaps: SmallVec<[i32; 8]> = granted_gaps.into();

    (sizes, gaps)
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

        let mut preferred_sizes = world.view::<&mut PreferredSize>();
        let mut preferred_sizes = preferred_sizes.lock().into();

        // Step 2: resolve each root and its subtree.

        for root in roots.iter() {
            measure_tree(root, world, &ui, &view, &mut sizes, &mut preferred_sizes);
        }
    }

    // Arrange phase.
    // Calculates the final rect of each widget based on its min size and the offered rect.
    {
        let mut view = world.view::<(
            &ResolvedAttributes,
            &MinSize,
            Option<&PreferredSize>,
            Option<&Container>,
        )>();
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
    use crate::{align::Align, style::Attributes};

    use super::*;

    fn r(num: i32, den: i32) -> Ratio {
        Ratio::new(num, NonZero::new(den).unwrap())
    }

    // --- Surplus (no shortage): plenty of room. Leftover routes to `Flexible`-stretch
    // children if any exist on the axis, else grows the gaps — never split evenly across
    // non-stretchy children (that's the reviewed fix for the reported "margin pops away"
    // bug: growth only ever comes from something that actually asked for it). ---

    #[test]
    fn distribute_case_a_equal_split() {
        // Neither child is `Flexible`-stretch, so the 10px surplus (container 30 - min_sum
        // 20) routes entirely into the 3 gap slots: 10/3 = 3 rem 1 -> [4, 3, 3] (remainder
        // biased to earlier gaps). Children themselves stay pinned at their bare min (10
        // each) — there's no `Relative`/margin demand to grant.
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let preferred_folded = [10, 10];
        let stretches = [false, false];
        // `container_stretches = true`: simulates a stretching container, so this pool-level
        // test still exercises the surplus-to-gaps fallback (the container-level gating added
        // by the shrink-to-fit fix is exercised separately by the `arrange_tree`-level tests).
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[0, 0, 0], 30, true);
        assert_eq!(sizes[..], [10, 10]);
        assert_eq!(gaps[..], [4, 3, 3]);
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 30);
    }

    #[test]
    fn distribute_case_a_remainder_biased_to_earlier_children() {
        // Same shape, zero mins this time: the full 10px surplus routes into the 4 gap
        // slots (no stretching children): 10/4 = 2 rem 2 -> [3, 3, 2, 2].
        let children = [(0, Ratio::ZERO), (0, Ratio::ZERO), (0, Ratio::ZERO)];
        let preferred_folded = [0, 0, 0];
        let stretches = [false, false, false];
        // `container_stretches = true`: see the comment in `distribute_case_a_equal_split`.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[0, 0, 0, 0], 10, true);
        assert_eq!(sizes[..], [0, 0, 0]);
        assert_eq!(gaps[..], [3, 3, 2, 2]);
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 10);
    }

    // --- Surplus with margins: the pool (gaps + size_extra + margin_extra) fits within
    // `available_extra` in full, so it's still a surplus — the true leftover then routes to
    // gaps (no stretching children in these two tests). ---

    #[test]
    fn distribute_case_a_margins_fit_use_full_gaps_and_max_collapse() {
        // Two children, both min=10, no relative demand, no margin beyond min
        // (preferred_folded == min for both): min_sum=20, available_extra=10. Pool =
        // natural_gaps [2,3,4] + size_extra [0,0] + margin_extra [0,0], sum=9 <=
        // available_extra(10) -> surplus of 1. Neither child stretches, so the 1px surplus
        // routes into the 3 gap slots (biased to the first): [3, 3, 4]. Children stay
        // pinned at their bare min (10 each) — nothing claims the leftover on their behalf.
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let preferred_folded = [10, 10];
        let stretches = [false, false];
        let natural_gaps = [2, 3, 4];
        // `container_stretches = true`: see the comment in `distribute_case_a_equal_split`.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &natural_gaps, 30, true);

        assert_eq!(sizes[..], [10, 10]);
        assert_eq!(gaps[..], [3, 3, 4]);

        // Sums-exactly-right: sizes + gaps == container_size.
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 30);
    }

    #[test]
    fn distribute_case_a_margins_shrink_when_they_dont_fully_fit() {
        // Same two children (min=10 each, no margin beyond min), but the natural gaps
        // (5+5+5=15) exceed `available_extra` (25-20=5): pool = [5,5,5,0,0,0,0] (gaps +
        // zero size_extra + zero margin_extra), sum=15 > 5 -> shortage.
        // `water_fill_shrink([5,5,5,0,0,0,0], 5)`: the four zeros grant trivially, leaving
        // `remaining_budget=5, remaining_count=3` for the three `5`s -> X = 5/3 = 1 rem 2,
        // so the gaps become [2, 2, 1] (remainder biased to the earlier gaps). Children
        // themselves are untouched by the shortage (their own `size_extra`/`margin_extra`
        // were already 0, so there was nothing to shrink there) — they stay at their bare
        // min (10 each).
        let children = [(10, Ratio::ZERO), (10, Ratio::ZERO)];
        let preferred_folded = [10, 10];
        let stretches = [false, false];
        let natural_gaps = [5, 5, 5];
        // Shortage regime — `container_stretches` is only consulted in the surplus branch, so
        // its value here is inert; kept `true` for consistency with the other pool tests.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &natural_gaps, 25, true);

        assert_eq!(sizes[..], [10, 10]);
        assert_eq!(gaps[..], [2, 2, 1]);

        // Sums-exactly-right: sizes + gaps == container_size, even in the shrunk case.
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 25);
    }

    // --- Shortage: the flat pool (gaps + `Relative` wants + margin-driven growth) doesn't
    // fully fit `available_extra` — the largest demands shrink first via `water_fill_shrink`,
    // a *leveling* water-fill, not a proportional one. Renamed from the old "Case B" tests:
    // there's no separate Case B anymore, this is just the shortage side of the unified
    // flat-pool regime (Case C, minimums themselves overflowing, is still separate). ---

    #[test]
    fn shortage_pool_favors_smaller_gaps_over_larger_relative_want() {
        // child 0: min=10, fully relative (rel=1, "wants" the whole container).
        // child 1: fixed-like, min=50, no relative demand.
        // No margin beyond min for either child.
        //
        // min_sum=60, available_extra=20. size_extra = [max(0,80-10)=70, max(0,0-50)=0] =
        // [70, 0]. margin_extra = [0, 0] (preferred_folded == min for both). Pool =
        // [3,4,5,70,0,0,0] (3 gaps + 2 size_extra + 2 margin_extra), sum=82 > 20 ->
        // shortage.
        //
        // `water_fill_shrink([3,4,5,70,0,0,0], 20)`: sorted ascending gives four zeros
        // first (three explicit zeros plus child1's own zero size_extra), then 3, 4, 5, then
        // 70 last. Granting 0,0,0,0,3,4,5 in turn (each passes the
        // `cap * remaining_count <= remaining_budget` check) leaves only the `70` ungranted,
        // with `remaining_budget = 20 - 3 - 4 - 5 = 8, remaining_count = 1` -> X = 8. So the
        // gaps stay `[3, 4, 5]` (all fully granted, unchanged, small enough to fit) while
        // child 0's oversized relative want gets clamped down from 70 to 8.
        let children = [(10, Ratio::int(1)), (50, Ratio::ZERO)];
        let preferred_folded = [10, 50];
        let stretches = [false, false];
        // Shortage regime — `container_stretches` is inert here (only consulted in the
        // surplus branch); see `distribute_case_a_margins_shrink_when_they_dont_fully_fit`.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[3, 4, 5], 80, true);
        assert_eq!(sizes[..], [18, 50]);
        assert_eq!(gaps[..], [3, 4, 5]);
        assert_eq!(sizes.iter().sum::<i32>() + gaps.iter().sum::<i32>(), 80);
    }

    #[test]
    fn shortage_pool_levels_competing_relative_children_without_weighting() {
        // child 0: no min, fully relative.
        // child 1: min=20, half-relative.
        // child 2: fixed-like, min=30, no relative demand. No margin beyond min anywhere.
        //
        // min_sum=50, available_extra=50. size_extra = [max(0,100-0)=100,
        // max(0,50-20)=30, max(0,0-30)=0] = [100, 30, 0]. margin_extra = [0,0,0]. Pool =
        // [0,0,0,0, 100,30,0, 0,0,0] (4 gaps + 3 size_extra + 3 margin_extra), sum=130 > 50
        // -> shortage.
        //
        // `water_fill_shrink`: the seven zeros grant trivially, leaving
        // `remaining_budget=50, remaining_count=2` for `[100, 30]` -> X = 25 (both exceed
        // 25, so both clamp to it). This demonstrates *leveling*, not proportional shrink:
        // child 0 and child 1 both end up granted exactly `+25` in absolute terms, even
        // though their original wants were 100 and 30 respectively (a 10:3 ratio, not
        // preserved) — an intentional, reviewed behavior change (weighted/proportional
        // shrink for competing `Relative` siblings is deferred to a future version), not a
        // regression.
        let children = [(0, Ratio::int(1)), (20, r(1, 2)), (30, Ratio::ZERO)];
        let preferred_folded = [0, 20, 30];
        let stretches = [false, false, false];
        // Shortage regime — `container_stretches` is inert here (only consulted in the
        // surplus branch); see `distribute_case_a_margins_shrink_when_they_dont_fully_fit`.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[0, 0, 0, 0], 100, true);
        assert_eq!(sizes[..], [25, 45, 30]);
        assert_eq!(gaps[..], [0, 0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 100);
    }

    // --- Case C: minimums themselves overflow; uniform proportional shrink (unaffected by
    // this task's changes — the flat-pool regime never engages here). ---

    #[test]
    fn distribute_case_c_proportional_shrink() {
        let children = [(60, Ratio::ZERO), (60, Ratio::ZERO)];
        let preferred_folded = [60, 60];
        let stretches = [false, false];
        // Case C — `container_stretches` is never consulted here (the flat-pool regime,
        // surplus or shortage, doesn't engage at all in Case C).
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[7, 7, 7], 90, true);
        assert_eq!(sizes[..], [45, 45]);
        // Margins are dropped entirely in Case C too.
        assert_eq!(gaps[..], [0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 90);
    }

    #[test]
    fn distribute_case_c_degenerate_zero_min_sum() {
        let children: [(i32, Ratio); 0] = [];
        let (sizes, gaps) = distribute_axis(&children, &[], &[], &[0], 90, true);
        assert_eq!(sizes[..], []);
        assert_eq!(gaps[..], [0]);
    }

    // --- `water_fill_shrink`: direct unit coverage. ---

    #[test]
    fn water_fill_shrink_worked_example() {
        // Caps [5, 2, 6], budget 10: cap 2 is small enough to grant in full immediately
        // (2*3=6 <= 10); the remaining budget (8) split evenly across the remaining two caps
        // ([5, 6], both exceeding 4) -> X = 4 for both.
        let result = water_fill_shrink(&[5, 2, 6], 10);
        assert_eq!(result[..], [4, 2, 4]);
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
        let mut view = world.view::<(
            &ResolvedAttributes,
            &MinSize,
            Option<&PreferredSize>,
            Option<&Container>,
        )>();
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

        // Column widths: min=[20,10] (cell0's `Fixed` override is unchanged, out of scope for
        // this task; cell1's own `MinSize` is its hand-set 10), rel=[0, 1/2], preferred_folded
        // = min (neither cell has its own inner_margin set), container width 100. min_sum=30,
        // available_extra=70. size_extra = [0, max(0,50-10)=40], margin_extra=[0,0]. Pool =
        // [0,0,0,0,40] (3 zero gaps + [0,40]), sum=40 <= 70 -> surplus of 30. Neither cell is
        // `Flexible`-stretch, so the 30px surplus routes into the 3 gap slots: 30/3=10 each ->
        // gaps=[10,10,10]. Column sizes: [20, 10+40] = [20, 50]. `cursor_x` starts at
        // `offer.lt.x=100`, `+= gaps[0]=10 -> 110`; cell0 occupies [110, 130); `+= gaps[1]=10
        // -> 140`; cell1 occupies [140, 190).
        let cell0_rect = arranged_rect(&mut world, cell0);
        assert_eq!(
            cell0_rect,
            Rect {
                lt: Pos { x: 110, y: 200 },
                rb: Pos { x: 130, y: 215 },
            }
        );

        // cell1's column slot is 50 wide (see above), and `Relative(1/2, 1)` caps its own size
        // at half of the *grid's* width (parent_rect, 100 -> 50), matching the slot exactly —
        // so it resolves to 50x50, filling its own slot.
        let cell1_rect = arranged_rect(&mut world, cell1);
        assert_eq!(
            cell1_rect,
            Rect {
                lt: Pos { x: 140, y: 200 },
                rb: Pos { x: 190, y: 250 },
            }
        );
    }

    #[test]
    fn flexible_shrink_to_fit_does_not_starve_children() {
        // A non-stretched Flexible container must lay its children out against the *real*
        // offer (not a bottom-up min_size, which is 0/unmeasured here in this hand-built
        // fixture) before shrinking to their occupied extent, so a Relative child inside it
        // still gets a real share of space rather than being squeezed to nothing. It must
        // ALSO not let leftover surplus (beyond what any child's own want/margin demands)
        // inflate the gaps on this axis, since gap growth counts toward the occupied extent
        // used to size a shrink-to-fit container — root is `Flexible{stretches:(false,false)}`,
        // so `distribute_axis`'s surplus-to-gaps fallback never engages here, and the
        // container ends up occupying only what its children truly need (70 of the 100-wide
        // offer), not the whole offer.
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

        // Width axis: min=[0,20] (fixed_child's `MinSize` mirrors its unchanged `Fixed`
        // override), rel=[1/2,0], preferred_folded=min (neither child has its own
        // inner_margin set), container 100. min_sum=20, available_extra=80. size_extra =
        // [max(0,50-0)=50, 0], margin_extra=[0,0]. Pool = [0,0,0,50,0,0] (3 zero gaps +
        // [50,0]), sum=50 <= 80 -> surplus of 30. Neither child is `Flexible`-stretch, AND
        // root itself is `Flexible{stretches:(false,false)}` (shrink-to-fit on width) — so the
        // 30px surplus goes unconsumed entirely (this test's whole point, post-fix): gaps stay
        // at their natural value `[0,0,0]` (no margin anywhere), sizes stay at their
        // fully-granted values `[0+50, 20+0] = [50, 20]`. `cursor_x` starts at 0, `+= gaps[0]=0
        // -> 0`; relative_child occupies [0, 50); `+= gaps[1]=0 -> 50`; fixed_child occupies
        // [50, 70).
        //
        // relative_child resolves `Relative(1/2, 1/4)` against `parent_rect` (100x40): w =
        // min(apply_ratio(100,1/2)=50, offer_w=50) = 50 (fills its slot exactly); cross axis
        // (height, no margin anywhere) h = min(apply_ratio(40,1/4)=10, offer_h=40) = 10.
        // fixed_child resolves to its own 20x10 regardless of slot, Start-aligned within [50,
        // 70).
        //
        // Occupied extent: occupied_w = max(50, 70) = 70, += trailing gap (gaps[2]=0) -> 70 —
        // the container now correctly occupies only 70 of the 100-wide offer (not the full
        // offer, and not the old "leftover split evenly" 85 either): with the surplus-to-gaps
        // fallback now gated on the container itself stretching, a shrink-to-fit container with
        // no stretching children never grows past what its children actually need.
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
                lt: Pos { x: 50, y: 0 },
                rb: Pos { x: 70, y: 10 },
            }
        );

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 70, y: 10 },
            }
        );
    }

    #[test]
    fn shrink_to_fit_axis_ignores_surplus_while_stretching_axis_still_fills_offer() {
        // Isolated proof of the fix in this task: a container that stretches on one axis but
        // not the other must gate the surplus-to-gaps fallback per axis, independently. Root
        // is `Flexible{stretches:(false, true)}` — shrink-to-fit width, stretch height — with
        // two plain (no stretch, no relative, no margin) `Fixed` children in a
        // `HorizontalStack`, offered far more width than they need.
        let mut world = World::new();

        let child_a = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 5 })),
                ..Default::default()
            },
            Size { w: 10, h: 5 },
        );
        let child_b = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 15, h: 5 })),
                ..Default::default()
            },
            Size { w: 15, h: 5 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (false, true),
                }),
                content_layout: Some(ContentLayout::HorizontalStack),
                ..Default::default()
            },
            Size::ZERO,
            vec![child_a, child_b],
        );
        world.get::<&mut Widget>(child_a).unwrap().parent = Some(root);
        world.get::<&mut Widget>(child_b).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 200, y: 50 },
        };
        run_arrange(&world, root, offer);

        // Width (main axis, HorizontalStack): min=[10,15], no relative/margin demand anywhere,
        // no natural gaps (no margin configured) -> min_sum=25, available_extra=175, pool
        // sums to 0 -> pure surplus of 175. Neither child stretches, AND root itself doesn't
        // stretch on width (`stretches.0 == false`) -> the surplus goes unconsumed: gaps stay
        // `[0,0,0]`, sizes stay `[10,15]`. occupied_w = 10+15 = 25, so root's own final width
        // is `25.min(200).max(0) = 25` — far less than the 200-wide offer.
        //
        // Height (cross axis of HorizontalStack, but root's own `stretches.1 == true`) always
        // resolves to the full offer height directly (see `arrange_tree`'s `if sy { offer_h }`
        // branch) — untouched by anything on the main axis.
        let root_rect = arranged_rect(&mut world, root);
        assert!(
            root_rect.rb.x - root_rect.lt.x < 200,
            "shrink-to-fit width axis should occupy less than the 200-wide offer, got {}",
            root_rect.rb.x - root_rect.lt.x
        );
        assert_eq!(root_rect.rb.x - root_rect.lt.x, 25);
        assert_eq!(root_rect.rb.y - root_rect.lt.y, 50); // Stretching axis still fills the offer.
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

        // Root is a `VerticalStack` (default `content_layout`), so width is the cross axis
        // (unaffected by this task, still shrinks to the child's own 30) while height is the
        // main axis: min_sum=0, available_extra=100, pool sums to 0 (no margin, no `Relative`
        // demand) -> pure surplus of 100. No stretching child AND root itself is
        // `Flexible{stretches:(false,false)}` (shrink-to-fit on height too) — so the surplus
        // goes unconsumed: gaps stay natural `[0, 0]`, not grown. The child's own height (20)
        // is the whole main-axis occupied extent (no gap growth added on top): occupied_h = 20.
        // So root's own shrink-to-fit size is 30 (width) x 20 (height), centered within the
        // 200x100 offer: offset = ((200-30)/2, (100-20)/2) = (85, 40).
        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 85, y: 40 },
                rb: Pos { x: 115, y: 60 },
            }
        );

        // The child, provisionally laid out at x=0, y=0 (no leading gap, since the surplus no
        // longer grows it) before the shift, must have been shifted by the same delta as
        // root's own origin move (85, 40) — a pure translation.
        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(
            child_rect,
            Rect {
                lt: Pos { x: 85, y: 40 },
                rb: Pos { x: 115, y: 60 },
            }
        );
    }

    #[test]
    fn stack_cross_axis_inset_uses_max_of_inner_and_outer_margin() {
        // HorizontalStack: the cross axis is height. The container's own `inner_margin`
        // (top=5, bottom=3) loses to the child's larger `outer_margin` (top=20) on top, but
        // wins over the child's smaller one (bottom=2) on the bottom (max-collapse per side,
        // independently) — so the child ends up inset by 20 from the top and 3 from the
        // bottom, not flush against either edge. `shrink_cross_inset` (cross-axis) is
        // unmodified by this task, so this part of the story is unchanged.
        //
        // The main axis (width) IS affected: no `inner_margin.left/right` and no child
        // `outer_margin.left/right` are set (all 0), and the single Fixed child has zero
        // `Relative` demand — min_sum=10, available_extra=90, pool sums to 0 -> pure surplus
        // of 90, routed entirely into the 2 gap slots (no stretching child): [45, 45]. The
        // child's own offer x-range becomes [45, 55), and (being `Fixed`) it ignores that
        // slot's size and Start-aligns its own 10x10 at its origin (x=45).
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
                lt: Pos { x: 45, y: 20 },
                rb: Pos { x: 55, y: 30 },
            }
        );
    }

    #[test]
    fn stack_cross_axis_inset_drops_entirely_when_content_min_fills_available_space() {
        // VerticalStack: the cross axis is width. `inner_margin.left` (8) alone already
        // exceeds the offered width (5), so `left_inset (8) + right_inset (0) = 8 >
        // available (5)`. The child is `Fixed(5, 5)`, so its own cross-axis minimum
        // (`child.min.w = 5`) already consumes the entire offered width on its own —
        // `leftover = available(5) - content_min(5) = 0`. Per `shrink_cross_inset`'s
        // Case C, content always gets priority over margin once the minimum itself
        // leaves no leftover space at all, so the inset is dropped entirely on both
        // sides: `shrink_cross_inset(8, 0, 5, 5)` returns `(0, 0)`, not some nonzero
        // shrunk value (the bug this test guards against).
        //
        // With `left_inset = 0` and `right_inset = 0`, the child's cross-axis offer is
        // the full `[0, 5)` span, flush against the root's left edge. The child itself
        // is `Fixed(5, 5)`, which (per `resolve_size`/`resolve_rect`) ignores the
        // offer's own size entirely and just aligns (Start, i.e. no offset) its fixed
        // 5x5 size at the offer's origin — so its final rect's X extends flush within
        // the 5-wide root with zero margin on either side.
        //
        // The main axis (height) IS affected: no `inner_margin.top/bottom` are set, and the
        // single Fixed child has zero `Relative` demand — min_sum=5, available_extra=15
        // (offer height 20), pool sums to 0 -> pure surplus of 15, routed entirely into the 2
        // gap slots (no stretching child): 15/2 = 7 rem 1 -> [8, 7]. The child's own offer
        // y-range starts at `cursor_y = 0 + gaps[0] = 8`, and (being `Fixed`) it Start-aligns
        // its own 5-tall size there.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 5, h: 5 })),
                ..Default::default()
            },
            Size { w: 5, h: 5 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(0, 8, 0, 0)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 5, y: 20 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(root_rect, offer);

        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(
            child_rect,
            Rect {
                lt: Pos { x: 0, y: 8 },
                rb: Pos { x: 5, y: 13 },
            }
        );
    }

    #[test]
    fn shrink_cross_inset_partial_shrink_when_content_min_leaves_some_leftover() {
        // VerticalStack: the cross axis is width. `inner_margin` is `Margin::new(0, 8, 0, 8)`
        // (left=8, right=8), so `left_inset (8) + right_inset (8) = 16`. The child's own
        // cross-axis minimum is `min.w = 2`, and the offered width is `10`, so
        // `leftover = available(10) - content_min(2) = 8` — less than the full inset sum
        // (16) but greater than 0, landing in `shrink_cross_inset`'s Case B (partial
        // shrink, not a full drop): `scale = leftover(8) / sum(16) = 1/2`, so
        // `left_inset = floor(8 * 1/2) = 4` and `right_inset = leftover(8) - 4 = 4` —
        // together summing to exactly `leftover` (8), leaving the child its full
        // `content_min` (2) worth of space.
        //
        // The child is `Fixed(2, 2)`, so (per `resolve_size`/`resolve_rect`) it ignores
        // the offer's own size and aligns (Start, i.e. no offset) its fixed 2x2 size at
        // the offer's origin: `x = rect.lt.x + left_inset = 0 + 4 = 4`, extending to
        // `x = 6`.
        //
        // The main axis (height) IS affected: no `inner_margin.top/bottom` are set, and the
        // single Fixed child has zero `Relative` demand — min_sum=2, available_extra=18
        // (offer height 20), pool sums to 0 -> pure surplus of 18, routed entirely into the 2
        // gap slots (no stretching child): 18/2 = 9 each -> [9, 9]. The child's own offer
        // y-range starts at `cursor_y = 0 + gaps[0] = 9`, and (being `Fixed`) it Start-aligns
        // its own 2-tall size there, extending to `y = 11`.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 2, h: 2 })),
                ..Default::default()
            },
            Size { w: 2, h: 2 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(0, 8, 0, 8)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 10, y: 20 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(root_rect, offer);

        let child_rect = arranged_rect(&mut world, child);
        assert_eq!(
            child_rect,
            Rect {
                lt: Pos { x: 4, y: 9 },
                rb: Pos { x: 6, y: 11 },
            }
        );
    }

    #[test]
    fn vertical_stack_shrink_to_fit_width_includes_trailing_cross_axis_inset() {
        // VerticalStack: the cross axis is width. A shrink-to-fit (non-stretched) width must
        // report back both the leading *and* trailing `inner_margin` inset around the child,
        // not just the leading one baked into the child's offer position.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 30, h: 12 })),
                ..Default::default()
            },
            Size { w: 30, h: 12 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (false, true),
                }),
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::uniform(8)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child],
        );
        world.get::<&mut Widget>(child).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 200, y: 150 },
        };
        run_arrange(&world, root, offer);

        // Width shrinks to left_inset(8) + child_width(30) + right_inset(8) = 46; height
        // stretches to fill the full 150-tall offer.
        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 46, y: 150 },
            }
        );
    }

    #[test]
    fn vertical_stack_shrink_to_fit_height_includes_trailing_main_axis_gap() {
        // VerticalStack: the main axis is height. A shrink-to-fit (non-stretched) height must
        // include the trailing `inner_margin.bottom` gap on top of the child's own extent, not
        // just the leading `inner_margin.top` gap already baked into the child's cursor start.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 15, h: 20 })),
                ..Default::default()
            },
            Size { w: 15, h: 20 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, false),
                }),
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(6, 0, 9, 0)),
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

        // Offered height (100) comfortably covers the child's min (20) plus the full top+bottom
        // margin (15): min_sum=20, available_extra=80. gaps=[6,9] (leading/trailing), pool
        // sums to 15 (no `Relative`/margin-beyond-min demand elsewhere) <= 80 -> surplus of
        // 65. Neither the child nor a sibling is `Flexible`-stretch on this axis, AND root
        // itself is `Flexible{stretches:(true,false)}` — shrink-to-fit on height, the main
        // axis here — so the 65px surplus goes unconsumed: gaps stay at their natural
        // leading/trailing value `[6, 9]`. `cursor_y` starts at 0, `+= gaps[0]=6 -> 6`; the
        // Fixed child occupies `[6, 26)`. occupied_h = max(0, 26-0) = 26, `+= gaps[1]=9 -> 35`
        // — this "shrink-to-fit" height now correctly occupies only 35 of the 100-tall offer
        // (child + both margins, nothing more), not the whole offer: with the surplus-to-gaps
        // fallback now gated on the container itself stretching, a shrink-to-fit axis with no
        // stretching children never grows past what it actually needs. Width stretches to the
        // full 100-wide offer regardless (unaffected, `stretches.0 == true`).
        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 100, y: 35 },
            }
        );
    }

    #[test]
    fn horizontal_stack_shrink_to_fit_height_includes_trailing_cross_axis_inset() {
        // HorizontalStack: the cross axis is height. Mirror of
        // `vertical_stack_shrink_to_fit_width_includes_trailing_cross_axis_inset`, but for the
        // other stack orientation's cross axis.
        let mut world = World::new();

        let child = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 18, h: 14 })),
                ..Default::default()
            },
            Size { w: 18, h: 14 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, false),
                }),
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::uniform(10)),
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

        // Width stretches to the full 100-wide offer; height shrinks to top_inset(10) +
        // child_height(14) + bottom_inset(10) = 34.
        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 100, y: 34 },
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

        // Row height: row min=max(10,10)=10, row_preferred_folded=max(10,10)=10 (no margin
        // beyond min on either cell), row_stretches=false (both `Fixed`). Row-aggregated
        // margins: top=max(cell0.top=6, cell1.top=9)=9, bottom=max(cell0.bottom=7,
        // cell1.bottom=1)=7. Leading gap=max(inner.top=2, 9)=9, trailing gap=max(7,
        // inner.bottom=4)=7. min_sum=10, available_extra=30 (content_h=40); pool=[9,7,0,0]
        // sums to 16 <= 30 -> surplus of 14, routed entirely into the 2 gap slots (no
        // stretching row): 14/2=7 each -> gaps=[16,14]. The row itself stays at its bare min
        // height (10) — nothing claims the surplus on its behalf. The row starts at
        // `cursor_y = 0 + gaps[0] = 16` and spans height 10 -> y=[16, 26).
        //
        // Column widths within the row: col min=[10,10], col_preferred_folded=[10,10] (no
        // margin beyond min), col_stretches=[false,false]. Cell-own (not aggregated) margins:
        // leading=max(inner.left=3, cell0.left=1)=3, between=max(cell0.right=2,
        // cell1.left=8)=8, trailing=max(cell1.right=0, inner.right=5)=5. min_sum=20,
        // available_extra=30 (content_w=50); pool=[3,8,5,0,0] sums to 16 <= 30 -> surplus of
        // 14, routed entirely into the 3 gap slots (no stretching cell): 14/3=4 rem 2 ->
        // gaps=[3+5=8, 8+5=13, 5+4=9]. Both columns stay at their bare min width (10).
        // `cursor_x` starts at `gaps[0]=8`; cell0 occupies x=[8, 18); `+= 10 -> 18`,
        // `+= gaps[1]=13 -> 31`; cell1 occupies x=[31, 41).
        let cell0_rect = arranged_rect(&mut world, cell0);
        assert_eq!(
            cell0_rect,
            Rect {
                lt: Pos { x: 8, y: 16 },
                rb: Pos { x: 18, y: 26 },
            }
        );

        let cell1_rect = arranged_rect(&mut world, cell1);
        assert_eq!(
            cell1_rect,
            Rect {
                lt: Pos { x: 31, y: 16 },
                rb: Pos { x: 41, y: 26 },
            }
        );
    }

    #[test]
    fn stack_cross_axis_inset_shared_across_siblings_not_per_child() {
        // VerticalStack: the cross axis is width. `inner_margin.left` is 8 (all other margins
        // 0), and there are two zero-outer-margin `Fixed` children: `child_a` (2x5, small
        // enough that, considered alone, `left_inset` would stay the full unshrunk 8), and
        // `child_b` (10x5, wide enough that, considered alone, it would force `left_inset` to
        // 0 per `shrink_cross_inset`'s Case C). The shared inset must be computed *once* for
        // the whole container, from the aggregated (max over all flow children) base and
        // content_min — not per child — so both children land at the same left edge.
        //
        // Base inset: left_inset_base = max(inner_margin.left=8, child_a.left=0,
        // child_b.left=0) = 8; right_inset_base = 0. Aggregated content_min_w = max(child_a.w=2,
        // child_b.w=10) = 10. `shrink_cross_inset(8, 0, available=10, content_min=10)`:
        // leftover = (10 - 10).max(0) = 0, and since `leftover <= 0`, Case C applies: the inset
        // is dropped entirely on both sides, `(0, 0)` — shared by every child, regardless of
        // that child's own min width.
        //
        // Main axis (height): both children are min 5, no relative demand, no margins on this
        // axis. min_sum=10, available_extra=40 (offer height 50), pool sums to 0 (no margin,
        // no `Relative` demand) -> pure surplus of 40, routed entirely into the 3 gap slots (no
        // stretching child): 40/3 = 13 rem 1 -> [14, 13, 13]. Both children stay at their bare
        // min height (5) — nothing claims the surplus on their behalf. `child_a` occupies y in
        // [14, 19) (`cursor_y` starts at `gaps[0]=14`); `child_b` occupies y in [32, 37)
        // (`cursor_y` advances by `child_a`'s height (5) then `gaps[1]=13` -> 14+5+13=32).
        //
        // With `left_inset = right_inset = 0` for both, each child's cross-axis offer spans the
        // full `[0, 10)` width. Both are `Fixed`, so (per `resolve_size`/`resolve_rect`) each
        // ignores the offer's own size and aligns (Start, i.e. no offset) its own fixed size at
        // the offer's origin: `x = rect.lt.x + left_inset = 0` for both, so their left edges
        // match exactly (the bug this test guards against: before the fix, `child_a` alone
        // would have kept the full unshrunk 8px inset and landed at `x = 8`, while `child_b`
        // landed at `x = 0` — misaligned).
        let mut world = World::new();

        let child_a = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 2, h: 5 })),
                ..Default::default()
            },
            Size { w: 2, h: 5 },
        );
        let child_b = spawn_widget(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 5 })),
                ..Default::default()
            },
            Size { w: 10, h: 5 },
        );

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(0, 8, 0, 0)),
                ..Default::default()
            },
            Size::ZERO,
            vec![child_a, child_b],
        );
        world.get::<&mut Widget>(child_a).unwrap().parent = Some(root);
        world.get::<&mut Widget>(child_b).unwrap().parent = Some(root);

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 10, y: 50 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(root_rect, offer);

        let child_a_rect = arranged_rect(&mut world, child_a);
        assert_eq!(
            child_a_rect,
            Rect {
                lt: Pos { x: 0, y: 14 },
                rb: Pos { x: 2, y: 19 },
            }
        );

        let child_b_rect = arranged_rect(&mut world, child_b);
        assert_eq!(
            child_b_rect,
            Rect {
                lt: Pos { x: 0, y: 32 },
                rb: Pos { x: 10, y: 37 },
            }
        );

        // Both children's left edges land at the same x, matching the shared shrunk inset —
        // not child_a sitting 8px further in than child_b.
        assert_eq!(child_a_rect.lt.x, root_rect.lt.x);
        assert_eq!(child_b_rect.lt.x, root_rect.lt.x);
    }

    // --- `None` (no explicit size) is pure sugar for `Flexible { stretches: (false, false) }`
    // — `resolve_size` no longer has a `None` arm at all; these tests exercise the unified
    // shrink-to-fit path via `arrange_tree`, which folds `inner_margin` into a leaf's occupied
    // size, then caps that occupied size at the offer before flooring it at `min_size`. This
    // means margin-driven growth only ever consumes spare room the offer actually has to
    // spare — it never pushes the widget past its slot — while content genuinely bigger than
    // `min_size` can still legitimately overflow the offer (that part is unchanged and applies
    // equally to a container's occupied children extent). This exactly mirrors how
    // `distribute_axis`'s Case A already gates outer-margin/gap growth between siblings on
    // available leftover space. ---

    #[test]
    fn no_explicit_size_grows_by_full_margin_when_offer_has_room() {
        // min = 10x10, inner_margin = uniform(4) -> margin.size() = 8x8 (4 per side, both
        // axes), so min + margin = 18x18. The offer (100x100) has plenty of spare room
        // beyond that, so the widget grows to exactly 18x18 (not the full offer).
        let mut world = World::new();

        let root = spawn_widget(
            &mut world,
            None,
            Attributes {
                inner_margin: Some(Margin::uniform(4)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 100, y: 100 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 18, y: 18 },
            }
        );
    }

    #[test]
    fn no_explicit_size_clamps_to_offer_when_margin_does_not_fully_fit() {
        // Same widget (min 10x10, margin.size() 8x8 -> min + margin = 18x18), but the offer
        // (15x15) is smaller than min + margin. Margin only grows into space the offer
        // actually has to spare, so the widget is clamped down to exactly the 15x15 offer
        // rather than the full min + margin — the min_size floor (10x10) doesn't kick in
        // since 15 is already >= 10.
        let mut world = World::new();

        let root = spawn_widget(
            &mut world,
            None,
            Attributes {
                inner_margin: Some(Margin::uniform(4)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 15, y: 15 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 15, y: 15 },
            }
        );
    }

    #[test]
    fn no_explicit_size_drops_margin_entirely_when_offer_has_no_spare_room() {
        // Offer exactly equals min_size (10x10, no spare room at all beyond min_size), so
        // margin is dropped entirely: min(18, 10).max(10) = 10. The widget resolves to
        // exactly min_size, the same size it would have without any margin at all.
        let mut world = World::new();

        let root = spawn_widget(
            &mut world,
            None,
            Attributes {
                inner_margin: Some(Margin::uniform(4)),
                ..Default::default()
            },
            Size { w: 10, h: 10 },
        );

        let offer = Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos { x: 10, y: 10 },
        };
        run_arrange(&world, root, offer);

        let root_rect = arranged_rect(&mut world, root);
        assert_eq!(
            root_rect,
            Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: 10, y: 10 },
            }
        );
    }

    // --- `PreferredSize`: the bottom-up "fully honored, no space pressure" size, computed
    // alongside `MinSize` in `measure_tree`. Unlike the tests above (which set `MinSize`
    // directly and only exercise `arrange_tree`), these exercise the actual bottom-up
    // measurement pass, so they build trees with no manually-injected `MinSize`/`PreferredSize`
    // and run the real `measure_tree` entry point via `run_measure`. ---

    fn spawn_leaf(world: &mut World, parent: Option<EntityId>, attrs: Attributes) -> EntityId {
        world
            .spawn((Widget { parent },))
            .insert(ResolvedAttributes(attrs))
            .unwrap()
            .id()
    }

    fn spawn_text_leaf(
        world: &mut World,
        parent: Option<EntityId>,
        attrs: Attributes,
        text: &str,
    ) -> EntityId {
        let id = spawn_leaf(world, parent, attrs);
        world
            .entity(id)
            .unwrap()
            .insert(Text::new(text.to_string()))
            .unwrap();
        id
    }

    fn spawn_measure_container(
        world: &mut World,
        parent: Option<EntityId>,
        attrs: Attributes,
        children: Vec<EntityId>,
    ) -> EntityId {
        let id = spawn_leaf(world, parent, attrs);
        world
            .entity(id)
            .unwrap()
            .insert(Container { children })
            .unwrap();
        id
    }

    /// Test-only equivalent of `layout_system`'s "Measure phase" block: ensures every widget
    /// has `Arranged`/`MinSize`/`PreferredSize` (mirroring `ensure_arranged_and_min_size`, the
    /// real entry point's own bootstrap step), then runs `measure_tree` directly against a
    /// `Ui::new()`, the same way `run_arrange` does for the arrange phase.
    fn run_measure(world: &mut World, root: EntityId) {
        ensure_arranged_and_min_size(world);

        let ui = Ui::new();
        let mut view = world.view::<(&ResolvedAttributes, Option<&Container>, Option<&Text>)>();
        let view = view.lock().into();
        let mut sizes = world.view::<&mut MinSize>();
        let mut sizes = sizes.lock().into();
        let mut preferred_sizes = world.view::<&mut PreferredSize>();
        let mut preferred_sizes = preferred_sizes.lock().into();

        let root = world.lookup(root).unwrap();
        measure_tree(root, world, &ui, &view, &mut sizes, &mut preferred_sizes);
    }

    fn preferred_size_of(world: &mut World, id: EntityId) -> PreferredSize {
        *world.get::<&PreferredSize>(id).unwrap()
    }

    #[test]
    fn measure_leaf_text_with_inner_margin() {
        // A leaf `Text` widget with no explicit `size`: `folded` grows the text's own
        // bounding box by the widget's own `inner_margin` (top=2, left=3, bottom=4, right=5
        // -> margin.size() = (3+5, 2+4) = (8, 6)).
        let mut world = World::new();
        let ui = Ui::new();
        let text = "Hi!";
        let text_size = ui.font(ui.default_font()).unwrap().text_bbox(text).size();

        let root = spawn_text_leaf(
            &mut world,
            None,
            Attributes {
                inner_margin: Some(Margin::new(2, 3, 4, 5)),
                ..Default::default()
            },
            text,
        );

        run_measure(&mut world, root);

        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, text_size + Margin::new(2, 3, 4, 5).size());
    }

    #[test]
    fn measure_leaf_fixed_size_ignores_content_and_margin() {
        // `WidgetSize::Fixed` overrides `folded` to the literal fixed value, ignoring the
        // text content and the `inner_margin` entirely.
        let mut world = World::new();

        let root = spawn_text_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 40, h: 20 })),
                inner_margin: Some(Margin::uniform(5)),
                ..Default::default()
            },
            "Hello, world!",
        );

        run_measure(&mut world, root);

        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, Size { w: 40, h: 20 });
    }

    #[test]
    fn measure_vertical_stack_folded() {
        // VerticalStack: main axis = height (sum + natural_gaps), cross axis = width (max,
        // plus aggregate_cross_inset).
        //
        // child_a: Fixed(w=10, h=20), outer_margin (top=1, left=4, bottom=2, right=5).
        // child_b: Fixed(w=15, h=25), outer_margin (top=3, left=6, bottom=1, right=2).
        // container inner_margin (top=5, left=8, bottom=6, right=9).
        //
        // Main axis (height): sum(child_folded_h) = 20 + 25 = 45.
        // natural_gaps(2, 5, 6, before=top, after=bottom): [max(5,1), max(2,3), max(1,6)]
        //   = [5, 3, 6] -> sum 14 -> folded.h = 45 + 14 = 59.
        //
        // Cross axis (width): max(10, 15) = 15.
        // left_inset = max(inner.left=8, 4, 6) = 8; right_inset = max(inner.right=9, 5, 2) = 9.
        // folded.w = 15 + 8 + 9 = 32.
        let mut world = World::new();

        let child_a = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 20 })),
                outer_margin: Some(Margin::new(1, 4, 2, 5)),
                ..Default::default()
            },
        );
        let child_b = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 15, h: 25 })),
                outer_margin: Some(Margin::new(3, 6, 1, 2)),
                ..Default::default()
            },
        );

        let root = spawn_measure_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(5, 8, 6, 9)),
                ..Default::default()
            },
            vec![child_a, child_b],
        );

        run_measure(&mut world, root);

        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, Size { w: 32, h: 59 });
    }

    #[test]
    fn measure_horizontal_stack_folded() {
        // HorizontalStack: exact axis-mirror of `measure_vertical_stack_folded` — main axis =
        // width, cross axis = height.
        //
        // child_a: Fixed(w=20, h=10), outer_margin (top=4, left=1, bottom=5, right=2).
        // child_b: Fixed(w=25, h=15), outer_margin (top=6, left=3, bottom=2, right=1).
        // container inner_margin (top=8, left=5, bottom=9, right=6).
        //
        // Main axis (width): sum(child_folded_w) = 20 + 25 = 45.
        // natural_gaps(2, 5, 6, before=left, after=right): [max(5,1), max(2,3), max(1,6)]
        //   = [5, 3, 6] -> sum 14 -> folded.w = 45 + 14 = 59.
        //
        // Cross axis (height): max(10, 15) = 15.
        // top_inset = max(inner.top=8, 4, 6) = 8; bottom_inset = max(inner.bottom=9, 5, 2) = 9.
        // folded.h = 15 + 8 + 9 = 32.
        let mut world = World::new();

        let child_a = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 20, h: 10 })),
                outer_margin: Some(Margin::new(4, 1, 5, 2)),
                ..Default::default()
            },
        );
        let child_b = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 25, h: 15 })),
                outer_margin: Some(Margin::new(6, 3, 2, 1)),
                ..Default::default()
            },
        );

        let root = spawn_measure_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::new(8, 5, 9, 6)),
                ..Default::default()
            },
            vec![child_a, child_b],
        );

        run_measure(&mut world, root);

        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, Size { w: 59, h: 32 });
    }

    #[test]
    fn measure_grid_row_aggregation_folded() {
        // A single Grid row with two cells, mirroring `grid_row_margin_aggregation_vs_cell_own_
        // margin`'s setup: row width goes through cell-own (unaggregated) margins, while row
        // height uses the row-aggregated (max over cells) top/bottom margin.
        //
        // cell0: Fixed(w=10, h=12), outer_margin (top=1, left=2, bottom=1, right=2).
        // cell1: Fixed(w=14, h=18), outer_margin (top=2, left=3, bottom=2, right=1).
        // container inner_margin (top=10, left=6, bottom=8, right=5).
        //
        // Row width: sum(cell.folded.w) = 10 + 14 = 24.
        // natural_gaps(2, 6, 5, before=left, after=right): [max(6,2), max(2,3), max(1,5)]
        //   = [6, 3, 5] -> sum 14 -> row_folded_w = 24 + 14 = 38 -> folded.w = 38.
        //
        // Row height: max(cell0.folded.h=12, cell1.folded.h=18) = 18.
        // Row-aggregated margins: top = max(1, 2) = 2, bottom = max(1, 2) = 2.
        // natural_gaps(1, 10, 8, before=row_top, after=row_bottom): [max(10,2), max(2,8)]
        //   = [10, 8] -> sum 18 -> folded.h = 18 + 18 = 36.
        let mut world = World::new();

        let cell0 = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 12 })),
                outer_margin: Some(Margin::new(1, 2, 1, 2)),
                ..Default::default()
            },
        );
        let cell1 = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 14, h: 18 })),
                outer_margin: Some(Margin::new(2, 3, 2, 1)),
                ..Default::default()
            },
        );

        let root = spawn_measure_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::Grid { rows: 1, cols: 2 }),
                inner_margin: Some(Margin::new(10, 6, 8, 5)),
                ..Default::default()
            },
            vec![cell0, cell1],
        );

        run_measure(&mut world, root);

        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, Size { w: 38, h: 36 });
    }

    #[test]
    fn measure_absolute_child_contributes_zero_to_parent_preferred_size() {
        // A VerticalStack with two flow children and one absolutely-positioned child appended
        // last (with zero outer_margin, and the container's own trailing `inner_margin.bottom`
        // also zero, so its zero-size/zero-margin presence doesn't perturb the main-axis gap
        // arithmetic below — isolating exactly the "absolute child contributes zero" effect this
        // test is about, rather than conflating it with the unrelated gap-collapsing rules
        // already covered by the stack tests above).
        //
        // child_a: Fixed(w=10, h=8), outer_margin (top=2, left=3, bottom=1, right=4).
        // child_b: Fixed(w=14, h=12), outer_margin (top=3, left=2, bottom=5, right=1).
        // absolute_child: position=Some(..), Fixed(w=100, h=100) (deliberately large — if it
        // wrongly contributed, the numbers below would be very different), outer_margin ZERO.
        // container inner_margin (top=5, left=6, bottom=0, right=7).
        //
        // Main axis (height), computed exactly as in `measure_vertical_stack_folded`, but with
        // the absolute child appended (zero size, zero margin, zero trailing inner_margin ->
        // its presence is a no-op):
        // sum(child_folded_h) = 8 + 12 + 0 = 20.
        // natural_gaps(3, 5, 0, ...): [max(5,2), max(1,3), max(5,0), max(0,0)] = [5, 3, 5, 0]
        //   -> sum 13 -> folded.h = 20 + 13 = 33.
        //
        // Cross axis (width): max(10, 14, 0) = 14 (the absolute child's zero folded.w never
        // raises the max). left_inset = max(inner.left=6, 3, 2, 0) = 6; right_inset =
        // max(inner.right=7, 4, 1, 0) = 7. folded.w = 14 + 6 + 7 = 27.
        let mut world = World::new();

        let child_a = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 8 })),
                outer_margin: Some(Margin::new(2, 3, 1, 4)),
                ..Default::default()
            },
        );
        let child_b = spawn_leaf(
            &mut world,
            None,
            Attributes {
                size: Some(WidgetSize::Fixed(Size { w: 14, h: 12 })),
                outer_margin: Some(Margin::new(3, 2, 5, 1)),
                ..Default::default()
            },
        );
        let absolute_child = spawn_leaf(
            &mut world,
            None,
            Attributes {
                position: Some(Pos { x: 1, y: 1 }),
                size: Some(WidgetSize::Fixed(Size { w: 100, h: 100 })),
                ..Default::default()
            },
        );

        let root = spawn_measure_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::VerticalStack),
                inner_margin: Some(Margin::new(5, 6, 0, 7)),
                ..Default::default()
            },
            vec![child_a, child_b, absolute_child],
        );

        run_measure(&mut world, root);

        // The absolute child's own `PreferredSize` is zeroed, matching `MinSize`'s existing
        // absolute-position convention.
        let absolute_pref = preferred_size_of(&mut world, absolute_child);
        assert_eq!(absolute_pref.folded, Size::ZERO);

        // ...and its parent's own `PreferredSize` is exactly as if the absolute child were
        // absent, per the arithmetic above.
        let pref = preferred_size_of(&mut world, root);
        assert_eq!(pref.folded, Size { w: 27, h: 33 });
    }

    // --- Arrange-time continuity: proving the flat water-fill pool has no discontinuous "pop"
    // like the old, now-deleted `MarginTier` system did (the reported bug: shrinking the
    // window, a root's entire margin vanishes at once instead of shrinking smoothly). These
    // tests hand-inject `PreferredSize` (via `spawn_widget`/`spawn_container` plus a direct
    // `.insert(PreferredSize { .. })`) for precise, hand-computed control. Unlike the old
    // `MarginTier` tests, these sweep along the *main* axis of a `HorizontalStack` (not the
    // cross axis) — that's the axis this task's flat-pool rewrite actually touches; the cross
    // axis (`shrink_cross_inset`) was already smooth on its own before `MarginTier` existed
    // (see the file-level context) and is untouched by this task beyond un-gating it. ---

    #[test]
    fn distribute_axis_root_margin_shrinks_continuously_no_discontinuous_pop() {
        // Root: HorizontalStack, own `inner_margin` (left=20, right=20). Two flow children,
        // each a plain leaf with a hand-set `MinSize`/`PreferredSize` where `folded == min`
        // (no margin overhead of their own) — isolating the exact claim under test: a child's
        // own rendered SIZE never changes regardless of offer width (the leaf's own
        // `.max(min_size)` clamp in `arrange_tree` guarantees this unconditionally), while the
        // root's own margin (unlike the old `MarginTier` system) shrinks in *small, bounded
        // steps* as the offer narrows, never vanishing all at once.
        //
        // min_sum = 15 + 25 = 40. natural_gaps(2, leading=20, trailing=20, before=[0,0],
        // after=[0,0]) = [20, 0, 20], sum 40. So: Case C (proportional shrink) for offer < 40;
        // the flat pool (shortage or surplus) for offer >= 40.
        let mut world = World::new();

        let child_a = spawn_widget(&mut world, None, Attributes::default(), Size { w: 15, h: 10 });
        world
            .entity(child_a)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 15, h: 10 },
            })
            .unwrap();

        let child_b = spawn_widget(&mut world, None, Attributes::default(), Size { w: 25, h: 10 });
        world
            .entity(child_b)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 25, h: 10 },
            })
            .unwrap();

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::new(0, 20, 0, 20)),
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                ..Default::default()
            },
            Size { w: 40, h: 10 },
            vec![child_a, child_b],
        );
        world
            .entity(root)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 80, h: 10 },
            })
            .unwrap();
        world.get::<&mut Widget>(child_a).unwrap().parent = Some(root);
        world.get::<&mut Widget>(child_b).unwrap().parent = Some(root);

        let root_left_inset_at = |world: &mut World, w: i32| -> i32 {
            let offer = Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: w, y: 50 },
            };
            run_arrange(world, root, offer);
            arranged_rect(world, child_a).lt.x - arranged_rect(world, root).lt.x
        };

        // Sweep the offer width 1px at a time from comfortably-plenty down to well below
        // min_sum, straddling every internal boundary (Case C <-> shortage at w=40, shortage
        // <-> surplus at w=80). At every single-pixel step, the root's own left inset changes
        // by at most 1px — the core continuity claim this test exists to prove.
        let mut prev: Option<i32> = None;
        for w in (10..=120).rev() {
            let offer = Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: w, y: 50 },
            };
            run_arrange(&world, root, offer);

            let a = arranged_rect(&mut world, child_a);
            let b = arranged_rect(&mut world, child_b);
            assert_eq!(a.rb.x - a.lt.x, 15, "child_a's own width changed at w={w}");
            assert_eq!(b.rb.x - b.lt.x, 25, "child_b's own width changed at w={w}");

            let inset = root_left_inset_at(&mut world, w);
            if let Some(prev_inset) = prev {
                let delta = (prev_inset - inset).abs();
                assert!(
                    delta <= 1,
                    "root's left inset jumped by {delta}px (from {prev_inset} to {inset}) \
                     going from offer width {} to {w} — this is the exact bug this test guards \
                     against",
                    w + 1
                );
            }
            prev = Some(inset);
        }

        // Spot-check a few exact values (hand-derived) as regression pins on top of the
        // continuity sweep above.
        assert_eq!(root_left_inset_at(&mut world, 200), 60); // Deep surplus: 20 + (120/3).
        assert_eq!(root_left_inset_at(&mut world, 80), 20); // Exactly at the shortage/surplus boundary.
        assert_eq!(root_left_inset_at(&mut world, 40), 0); // Exactly at the Case-C/shortage boundary.
        assert_eq!(root_left_inset_at(&mut world, 20), 0); // Deep in Case C: margin dropped entirely.
    }

    #[test]
    fn nested_container_margin_shrinks_continuously_no_discontinuous_pop() {
        // The original bug's exact shape: a root whose own `inner_margin` is bigger than its
        // (nested container) child's own `inner_margin`. Structure: `root` (HorizontalStack,
        // Flexible(true, true), inner_margin left=30/right=30) -> `inner` (HorizontalStack,
        // inner_margin left=8/right=8) -> `g` (a leaf, MinSize/PreferredSize.folded = (40, 10),
        // no margin overhead of its own). Both `root` and `inner` are `HorizontalStack`s so
        // sweeping the offer's WIDTH exercises the flat-pool main axis at *both* nesting
        // levels — a stronger version of the original bug repro, which (being a
        // `VerticalStack`) actually swept the cross axis, never the axis this task's rewrite
        // touches.
        //
        //   g.folded = (40, 10) (no margin of its own).
        //   inner.folded.w = 40 + inner's own left/right inset (8 + 8) = 56.
        //   root.folded.w = 56 + root's own left/right inset (30 + 30) = 116.
        let mut world = World::new();

        let g = spawn_widget(&mut world, None, Attributes::default(), Size { w: 40, h: 10 });
        world
            .entity(g)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 40, h: 10 },
            })
            .unwrap();

        let inner = spawn_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::new(0, 8, 0, 8)),
                ..Default::default()
            },
            Size { w: 40, h: 10 },
            vec![g],
        );
        world
            .entity(inner)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 56, h: 10 },
            })
            .unwrap();
        world.get::<&mut Widget>(g).unwrap().parent = Some(inner);

        let root = spawn_container(
            &mut world,
            None,
            Attributes {
                content_layout: Some(ContentLayout::HorizontalStack),
                inner_margin: Some(Margin::new(0, 30, 0, 30)),
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                ..Default::default()
            },
            Size { w: 40, h: 10 },
            vec![inner],
        );
        world
            .entity(root)
            .unwrap()
            .insert(PreferredSize {
                folded: Size { w: 116, h: 10 },
            })
            .unwrap();
        world.get::<&mut Widget>(inner).unwrap().parent = Some(root);

        let measure_at = |world: &mut World, w: i32| -> (i32, i32, i32) {
            let offer = Rect {
                lt: Pos { x: 0, y: 0 },
                rb: Pos { x: w, y: 50 },
            };
            run_arrange(world, root, offer);
            let root_rect = arranged_rect(world, root);
            let inner_rect = arranged_rect(world, inner);
            let g_rect = arranged_rect(world, g);
            (
                g_rect.rb.x - g_rect.lt.x,        // g's own width — must always stay 40.
                inner_rect.lt.x - root_rect.lt.x, // root's own left inset.
                g_rect.lt.x - inner_rect.lt.x,    // inner's own left inset.
            )
        };

        // Sweep the offer width 1px at a time across a wide range spanning "plenty of room"
        // (well above 116) down to "tight" (well below 40). At every step: `g`'s own width
        // never changes (content protected), and neither inset changes by more than a small,
        // bounded amount (2px, allowing for the composition of two independently-rounding
        // nesting levels) — the core proof that nesting two levels of the flat-pool water-fill
        // still degrades smoothly, unlike the old `MarginTier` system's all-at-once drop.
        let mut prev: Option<(i32, i32, i32)> = None;
        for w in (10..=160).rev() {
            let (g_w, root_inset, inner_inset) = measure_at(&mut world, w);
            assert_eq!(g_w, 40, "g's own width changed at offer width {w}");

            if let Some((_, prev_root_inset, prev_inner_inset)) = prev {
                let root_delta = (prev_root_inset - root_inset).abs();
                let inner_delta = (prev_inner_inset - inner_inset).abs();
                assert!(
                    root_delta <= 2,
                    "root's left inset jumped by {root_delta}px (from {prev_root_inset} to \
                     {root_inset}) going from offer width {} to {w}",
                    w + 1
                );
                assert!(
                    inner_delta <= 2,
                    "inner's left inset jumped by {inner_delta}px (from {prev_inner_inset} to \
                     {inner_inset}) going from offer width {} to {w}",
                    w + 1
                );
            }
            prev = Some((g_w, root_inset, inner_inset));
        }

        // Spot-check a few exact values (hand-derived) as regression pins on top of the
        // continuity sweep above.
        //
        // `inner` is not `Flexible`-stretch, so once root's own pool (gaps [30,30] +
        // `inner`'s margin_extra, capped at its own preferred_folded - min = 16) is fully
        // granted, any further offer growth keeps routing into ROOT's own gaps (nothing else
        // claims it) — root's own inset keeps growing without bound as the offer grows, same
        // story as the single-level test above; it does NOT "cap" at the bare 30 once
        // `inner`'s own preferred size is satisfied.
        let (_, root_inset, inner_inset) = measure_at(&mut world, 200);
        // available_extra=160, pool=[30,30,0,16] (sum 76) -> surplus=84, routed into root's 2
        // gap slots (inner isn't stretch): 84/2=42 each -> gaps=[72,72].
        assert_eq!(root_inset, 72);
        // inner's own slot from root = min(40) + margin_extra(16, fully granted) = 56 = inner's
        // own preferred_folded exactly -> inner's own internal surplus is 0, its own gaps stay
        // at their natural [8,8].
        assert_eq!(inner_inset, 8);

        let (_, root_inset, inner_inset) = measure_at(&mut world, 116);
        // Exactly at root's own pool-fully-granted boundary: surplus=0, gaps=[30,30].
        assert_eq!(root_inset, 30);
        assert_eq!(inner_inset, 8);

        let (_, root_inset, _) = measure_at(&mut world, 40);
        assert_eq!(root_inset, 0); // At root's own bare min_sum boundary: root's margin is 0.
    }

    // --- Rebaselining `margin_extra` against `min + size_extra` (not bare `min`) prevents
    // double-counting a `Relative` child's own want and its own margin-driven growth. ---

    #[test]
    fn margin_extra_is_baselined_against_want_adjusted_floor_not_bare_min() {
        // Single child: min=10, rel=1/2, container=100 -> want = floor(100 * 1/2) = 50.
        // `preferred_folded` = 30: bigger than bare min (10), but smaller than the want (50).
        //
        // size_extra = max(0, want(50) - min(10)) = 40.
        // margin_extra, baselined against `min + size_extra` (= 50, not bare min 10) =
        //   max(0, preferred_folded(30) - min(10) - size_extra(40)) = max(0, -20) = 0.
        //
        // Without the rebaseline fix (baselining against bare `min` instead), margin_extra
        // would incorrectly be `max(0, 30 - 10) = 20`, double-counting the same 20px of
        // "growth toward 30" that `size_extra` (40) already fully covers and then some —
        // pool sum would be 60 instead of 40, and the child would over-grow to 70, exceeding
        // both its want (50) and its preferred_folded (30).
        //
        // Pool = natural_gaps [0,0] + size_extra [40] + margin_extra [0], sum=40. min_sum=10,
        // available_extra=90 >= 40 -> surplus of 50. No stretch child -> surplus routes into
        // the 2 gap slots: [25, 25]. Final size = min + size_extra + margin_extra = 10+40+0 =
        // 50 = max(want=50, preferred_folded=30), exactly as expected — never overshooting.
        let children = [(10, r(1, 2))];
        let preferred_folded = [30];
        let stretches = [false];
        // `container_stretches = true`: simulates a stretching container so the true 50px
        // leftover (beyond the 40px pool) still routes into the gaps, exercising the pool
        // baseline math under test here rather than the (separately tested) shrink-to-fit
        // gate.
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[0, 0], 100, true);

        assert_eq!(sizes[..], [50]);
        assert_eq!(gaps[..], [25, 25]);
        assert_eq!(sizes[0], 50i32.max(30)); // max(want, preferred_folded).
    }

    // --- Surplus routes to `Flexible{stretches}` children first, not evenly across all
    // children — the reviewed fix for the "surplus silently disappears into unused slot
    // padding" bug (see `flexible_shrink_to_fit_does_not_starve_children` above). ---

    #[test]
    fn surplus_routes_to_stretching_child_not_fixed_sibling() {
        // child 0: min=0, no relative demand, `Flexible{stretches:(true, _)}` on this axis.
        // child 1: min=20, no relative demand, not stretching (e.g. `Fixed`/plain).
        // container=100. min_sum=20, available_extra=80. Pool (no margin, no rel demand) sums
        // to 0 -> surplus of 80, routed entirely to the one stretching child (index 0).
        let children = [(0, Ratio::ZERO), (20, Ratio::ZERO)];
        let preferred_folded = [0, 20];
        let stretches = [true, false];
        // `container_stretches = false`: deliberately the opposite of the "stretching
        // container" simulation used elsewhere — proves a stretching *child* still absorbs
        // surplus unconditionally, even inside a container that is itself shrink-to-fit on
        // this axis (see `distribute_axis`'s doc comment).
        let (sizes, gaps) =
            distribute_axis(&children, &preferred_folded, &stretches, &[0, 0, 0], 100, false);

        assert_eq!(sizes[..], [80, 20]);
        assert_eq!(gaps[..], [0, 0, 0]);
        assert_eq!(sizes.iter().sum::<i32>(), 100);
    }
}
