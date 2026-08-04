use std::num::NonZero;

use edict::{
    component::Component,
    entity::EntityId,
    query::{Entities, Or, Without},
    view::View,
    world::World,
};
use smallvec::{SmallVec, smallvec};

use crate::{
    align::Align2,
    margin::Margin,
    math::{Pos, Ratio, Rect, Size, Vec},
    style::{LayoutAttributes, Stretch, WidgetPos, WidgetSize},
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

    /// Widgets are arranged in a grid with a fixed number of rows; the
    /// number of columns is derived from the child count, filling row-major
    /// (i.e. row-by-row) so `rows` fills up first: `cols = ceil(children /
    /// rows)`.
    GridRows { rows: u32 },

    /// Widgets are arranged in a grid with a fixed number of columns; the
    /// number of rows is derived from the child count: `rows = ceil(children
    /// / cols)`.
    GridCols { cols: u32 },
}

/// The intermediate size asked by a widget from its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct Measure {
    /// The minimum size of the widget required to display its content.
    pub min: Size,

    /// The preferred size of the widget, including margins and additional space for its content.
    pub preferred: Size,

    /// Whether the widget stretches to fill the available space in each axis.
    pub stretches: Stretch,
}

/// The rect of a widget calculated by the layout system, after measuring and arranging its content and children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct Arranged {
    pub rect: Rect,
    pub layer: u32,
}

/// Ensures that widgets has required components for layouting: [`Arranged`], [`MinSize`] and [`PreferredSize`].
fn ensure_arranged_and_measure(world: &mut World) {
    let world = world.local();
    let view = world
        .view::<(Entities, Option<&Arranged>, Option<&Measure>)>()
        .with::<Widget>()
        .filter(Or::<(Without<Arranged>, Without<Measure>)>::default());

    let default_arranged = Arranged {
        rect: Rect::ZERO,
        layer: 0,
    };
    let default_measure = Measure {
        min: Size::ZERO,
        preferred: Size::ZERO,
        stretches: Stretch::NONE,
    };

    // Inserts are suboptimal, but are not expected to run frequently.
    // It only supposed to run for new widgets once.
    for (e, arranged, measure) in view {
        match (arranged, measure) {
            (Some(_), Some(_)) => {
                // Both are already present.
                // Unreachable because the view filters for entities that are missing at least one of the components.
                unreachable!();
            }
            (Some(_), None) => {
                world.insert_defer(e, default_measure);
            }
            (None, Some(_)) => {
                world.insert_defer(e, default_arranged);
            }
            (None, None) => {
                world.insert_bundle_defer(e, (default_arranged, default_measure));
            }
        }
    }

    world.run_deferred();
}

/// Resolves the [`Rect`] layout for every [`Widget`] entity, storing the result
/// in a [`ResolvedRect`] component.
///
/// # Precondition
///
/// Every `Widget` entity must already have a [`FinalAttributes`] component with
/// theme/fallback merging applied, i.e. this must run after `Style::resolve_attributes`.
pub fn layout_system(world: &mut World) {
    // Ensure that every widget has both an `Arranged` and a `Measure` component.
    ensure_arranged_and_measure(world);

    let Some(ui) = world.get_resource::<Ui>() else {
        return;
    };

    let roots = world.view::<Entities>().with::<RootWidget>();

    // Measure phase.
    // Calculates the minimum size of each widget.
    // For containers min size is calculated based on the min size of its children.
    {
        let mut view = world.view::<(&LayoutAttributes, Option<&Container>, Option<&Text>)>();
        let view = view.lock().into();

        let mut sizes = world.view::<&mut Measure>();
        let mut sizes = sizes.lock().into();

        // Step 2: resolve each root and its subtree.

        for root in roots.iter() {
            measure_widget(root.id(), &view, &mut sizes, &ui);
        }
    }

    // Arrange phase.
    // Calculates the final rect of each widget based on its min size and the offered rect.
    {
        let mut view = world.view::<(&LayoutAttributes, &Measure, Option<&Container>)>();
        let view = view.lock().into();

        let mut arranged = world.view::<&mut Arranged>();
        let mut arranged = arranged.lock().into();

        // Step 2: resolve each root and its subtree.

        for root in roots.iter() {
            arrange_root(root.id(), &view, &mut arranged, &ui);
        }
    }
}

pub(crate) fn shrink_cross_inset(
    leading: i32,
    trailing: i32,
    available: i32,
    content_min: i32,
) -> (i32, i32) {
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
    let leading = (leading * scale).floor();
    let trailing = leftover - leading;

    (leading, trailing)
}

// ============================================================================
// Measure/arrange engine.
//
// See the module-level design notes in the layout spec this was implemented
// against: every widget's `Rect` is fully decided by its parent before the
// widget's own arrange step begins (the "core invariant"). A `Line` is the
// 1-D analogue of `Measure` (min/preferred/stretch + leading/trailing
// margin), used identically for rows and columns at every level of the tree,
// including the degenerate 1xN / Nx1 grids that `HorizontalStack` /
// `VerticalStack` reduce to, and the degenerate 1x1 grid that root (and
// absolutely-positioned widgets) reduce to.
// ============================================================================

/// The 1-D analogue of [`Measure`]: a row's or column's own aggregated
/// metrics, plus the leading/trailing outer margin used for gap computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Line {
    min: i32,
    preferred: i32,
    stretch: bool,
    margin_leading: i32,
    margin_trailing: i32,
}

const EMPTY_LINE: Line = Line {
    min: 0,
    preferred: 0,
    stretch: false,
    margin_leading: 0,
    margin_trailing: 0,
};

/// Aggregates two `Line`s meeting at the same row/column: this is the whole
/// of the "grid aggregation" rule (max/OR/max), applied identically whether
/// aggregating over a single row's cells or over an entire column's cells
/// across every row.
fn merge_line(a: Line, b: Line) -> Line {
    Line {
        min: a.min.max(b.min),
        preferred: a.preferred.max(b.preferred),
        stretch: a.stretch || b.stretch,
        margin_leading: a.margin_leading.max(b.margin_leading),
        margin_trailing: a.margin_trailing.max(b.margin_trailing),
    }
}

/// A single cell's own contribution to the row axis (height dimension),
/// derived from its own `Measure` and outer margin. `preferred` is clamped
/// to be at least `min` so every `Line` fed into the axis algorithm upholds
/// that invariant unconditionally (relevant once the `Relative` override,
/// applied by the caller, can otherwise push `preferred` below `min`).
fn cell_line_row(measure: Measure, margin: Margin) -> Line {
    Line {
        min: measure.min.h,
        preferred: measure.preferred.h.max(measure.min.h),
        stretch: measure.stretches.vertical,
        margin_leading: margin.top as i32,
        margin_trailing: margin.bottom as i32,
    }
}

/// Symmetric to [`cell_line_row`], for the column axis (width dimension).
fn cell_line_col(measure: Measure, margin: Margin) -> Line {
    Line {
        min: measure.min.w,
        preferred: measure.preferred.w.max(measure.min.w),
        stretch: measure.stretches.horizontal,
        margin_leading: margin.left as i32,
        margin_trailing: margin.right as i32,
    }
}

/// Computes the natural (max-collapsed) leading/between/trailing gaps for a
/// sequence of `Line`s: `gaps.len() == items.len() + 1`.
fn natural_gaps(items: &[Line], leading: i32, trailing: i32) -> SmallVec<[i32; 9]> {
    let n = items.len();
    if n == 0 {
        return smallvec![leading.max(trailing)];
    }

    let mut gaps: SmallVec<[i32; 9]> = SmallVec::with_capacity(n + 1);
    gaps.push(leading.max(items[0].margin_leading));
    for i in 1..n {
        gaps.push(items[i - 1].margin_trailing.max(items[i].margin_leading));
    }
    gaps.push(items[n - 1].margin_trailing.max(trailing));

    gaps
}

/// `(S_min, S_pref)` for a sequence of `Line`s and their container's own
/// leading/trailing inset: the same sums the axis-distribution algorithm
/// uses for its regime boundaries, exposed standalone so the bottom-up
/// measure pass can compute a container's own reported min/preferred with
/// the exact same arithmetic the arrange pass will later use.
fn sum_min_pref(items: &[Line], leading: i32, trailing: i32) -> (i32, i32) {
    let gaps = natural_gaps(items, leading, trailing);
    let s_min: i32 = items.iter().map(|it| it.min).sum();
    let s_pref: i32 = items.iter().map(|it| it.preferred).sum::<i32>() + gaps.iter().sum::<i32>();
    (s_min, s_pref)
}

/// Result of [`distribute_axis`]: the resolved size of each item (row or
/// column `Line`) and each gap around/between them (`gaps.len() ==
/// item_sizes.len() + 1`).
struct AxisLayout {
    item_sizes: SmallVec<[i32; 8]>,
    gaps: SmallVec<[i32; 9]>,
}

/// The one axis-distribution algorithm, used for rows and (independently)
/// for columns, by every container. See the module doc / design spec for
/// the three regimes; this is a direct implementation of them.
fn distribute_axis(
    items: &[Line],
    container_leading: i32,
    container_trailing: i32,
    container_stretches: bool,
    size: i32,
) -> AxisLayout {
    let n = items.len();

    if n == 0 {
        return AxisLayout {
            item_sizes: SmallVec::new(),
            gaps: smallvec![size.max(0)],
        };
    }

    let gaps_natural = natural_gaps(items, container_leading, container_trailing);

    let s_pref: i32 =
        items.iter().map(|it| it.preferred).sum::<i32>() + gaps_natural.iter().sum::<i32>();
    let s_min: i32 = items.iter().map(|it| it.min).sum();

    let any_item_stretches = items.iter().any(|it| it.stretch);
    let has_consumer = any_item_stretches || container_stretches;

    let effective_size = if has_consumer { size } else { size.min(s_pref) };

    if effective_size >= s_pref {
        // Regime 1: surplus (or the `effective_size == s_pref` boundary, which
        // is also where a `!has_consumer` container always lands).
        let mut item_sizes: SmallVec<[i32; 8]> = items.iter().map(|it| it.preferred).collect();
        let mut gaps = gaps_natural.clone();

        let surplus = effective_size - s_pref;
        if surplus > 0 {
            if any_item_stretches {
                let stretch_idx: SmallVec<[usize; 8]> = items
                    .iter()
                    .enumerate()
                    .filter(|(_, it)| it.stretch)
                    .map(|(i, _)| i)
                    .collect();
                let shares = distribute_remainder(surplus, stretch_idx.len());
                for (&idx, share) in stretch_idx.iter().zip(shares) {
                    item_sizes[idx] += share;
                }
            } else {
                debug_assert!(
                    container_stretches,
                    "surplus with no consumer is unreachable by construction"
                );
                let shares = distribute_remainder(surplus, n + 1);
                for (g, share) in gaps.iter_mut().zip(shares) {
                    *g += share;
                }
            }
        }

        AxisLayout { item_sizes, gaps }
    } else if effective_size >= s_min {
        // Regime 2: continuous shrink from preferred toward min.
        let budget = effective_size - s_min;

        let mut caps: SmallVec<[i32; 17]> = SmallVec::with_capacity(2 * n + 1);
        for it in items {
            caps.push((it.preferred - it.min).max(0));
        }
        caps.extend(gaps_natural.iter().copied());

        let grants = water_fill_shrink(&caps, budget);

        let item_sizes: SmallVec<[i32; 8]> = (0..n).map(|k| items[k].min + grants[k]).collect();
        let gaps: SmallVec<[i32; 9]> = (0..=n).map(|i| grants[n + i]).collect();

        AxisLayout { item_sizes, gaps }
    } else {
        // Regime 3: shrink below min. Gaps are already zero.
        let budget = effective_size.max(0);
        let caps: SmallVec<[i32; 8]> = items.iter().map(|it| it.min).collect();
        let grants = water_fill_shrink(&caps, budget);

        AxisLayout {
            item_sizes: grants.into_iter().collect(),
            gaps: smallvec![0; n + 1],
        }
    }
}

/// A saturating water-fill: given non-negative `caps` and `budget <=
/// sum(caps)`, finds `X` such that `sum(min(cap_i, X)) == budget`. Smallest
/// caps are preserved unchanged the longest; the largest caps give way
/// (clamp down to `X`) first. Implemented via a sort + running-count sweep.
fn water_fill_shrink(caps: &[i32], budget: i32) -> SmallVec<[i32; 16]> {
    let n = caps.len();
    let mut result: SmallVec<[i32; 16]> = smallvec![0; n];
    if n == 0 {
        return result;
    }

    let budget = budget.max(0);

    let mut order: SmallVec<[usize; 16]> = (0..n).collect();
    order.sort_by_key(|&i| caps[i]);

    let mut remaining_budget: i64 = budget as i64;
    let mut cursor = 0usize;

    while cursor < n {
        let idx = order[cursor];
        let remaining_count = (n - cursor) as i64;
        let cap = caps[idx] as i64;

        // An even split of the remaining budget across the remaining
        // (not-yet-granted) caps would not exceed this, the smallest
        // remaining cap: grant it in full and move on.
        if cap * remaining_count <= remaining_budget {
            result[idx] = caps[idx];
            remaining_budget -= cap;
            cursor += 1;
        } else {
            break;
        }
    }

    if cursor < n {
        let remaining_count = (n - cursor) as i64;
        let level = remaining_budget / remaining_count;
        let mut remainder = (remaining_budget % remaining_count) as usize;

        for &idx in &order[cursor..] {
            let extra = if remainder > 0 {
                remainder -= 1;
                1
            } else {
                0
            };
            result[idx] = (level + extra) as i32;
        }
    }

    result
}

/// Splits `total` (assumed `>= 0`) evenly across `n` items: base +1 for the
/// first `total % n` items in iteration order.
fn distribute_remainder(total: i32, n: usize) -> SmallVec<[i32; 8]> {
    let mut out: SmallVec<[i32; 8]> = smallvec![0; n];
    if n == 0 {
        return out;
    }

    let n = n as i32;
    let base = total / n;
    let remainder = (total % n) as usize;

    for (i, slot) in out.iter_mut().enumerate() {
        *slot = base + if i < remainder { 1 } else { 0 };
    }

    out
}

/// Applies a [`Ratio`] to an integer pixel value, flooring the result.
fn apply_ratio(value: i32, ratio: Ratio) -> i32 {
    (value * ratio).floor()
}

/// Resolves a single dimension of a widget's own size against a granted
/// slot: full slot if it stretches on this axis, otherwise its own
/// preferred value clamped to the slot (never exceeding it, never going
/// negative). This is the "per-cell, within an already-resolved slot" step
/// — distinct from [`distribute_axis`], which resolves the *slot* itself.
fn resolve_size_component(preferred: i32, stretch: bool, available: i32) -> i32 {
    if stretch {
        available.max(0)
    } else {
        preferred.min(available).max(0)
    }
}

/// Resolves the size of an absolutely-positioned widget (flow-item or not):
/// `Fixed` is exact, `Relative` is resolved against `own_size` (the
/// immediate parent's own already-known size on each axis, per the core
/// invariant), and anything else (unset, or `Flexible`, which has no slot to
/// stretch into here) just uses the widget's own measured preferred size.
fn resolve_absolute_size(layout: &LayoutAttributes, measure: Measure, own_size: Size) -> Size {
    match layout.size {
        Some(WidgetSize::Fixed(size)) => size,
        Some(WidgetSize::Relative(rw, rh)) => Size {
            w: apply_ratio(own_size.w, rw).max(measure.min.w),
            h: apply_ratio(own_size.h, rh).max(measure.min.h),
        },
        _ => measure.preferred,
    }
}

/// Given an [`AxisLayout`], returns the starting offset (relative to the
/// container's own origin on this axis) of each item.
fn axis_offsets(layout: &AxisLayout) -> SmallVec<[i32; 8]> {
    let mut offsets: SmallVec<[i32; 8]> = SmallVec::with_capacity(layout.item_sizes.len());
    let mut cursor = layout.gaps.first().copied().unwrap_or(0);
    for i in 0..layout.item_sizes.len() {
        offsets.push(cursor);
        cursor += layout.item_sizes[i] + layout.gaps[i + 1];
    }
    offsets
}

/// The final per-cell placement step: resolves the cell widget's own size
/// within its granted `slot` (stretch-fill or clamp-to-preferred) and aligns
/// it within that slot.
fn place_in_slot(row_line: Line, col_line: Line, slot: Rect, align: Align2) -> Rect {
    let slot_size = slot.size();
    let size = Size {
        w: resolve_size_component(col_line.preferred, col_line.stretch, slot_size.w),
        h: resolve_size_component(row_line.preferred, row_line.stretch, slot_size.h),
    };
    align.in_rect(slot, size)
}

/// The row/column shape of a container's content layout: `Grid` natively,
/// `HorizontalStack` = 1xN, `VerticalStack` = Nx1, `GridRows`/`GridCols`
/// derive their other dimension from the child count via ceiling division
/// (`size2 = ceil(children / size1)`), a zero fixed dimension degenerating
/// to an empty (0x0) grid rather than dividing by zero.
fn grid_shape(content_layout: ContentLayout, n_children: usize) -> (usize, usize) {
    match content_layout {
        ContentLayout::VerticalStack => (n_children, 1),
        ContentLayout::HorizontalStack => (1, n_children),
        ContentLayout::Grid { rows, cols } => (rows as usize, cols as usize),
        ContentLayout::GridRows { rows } => {
            let rows = rows as usize;
            if rows == 0 {
                (0, 0)
            } else {
                (rows, n_children.div_ceil(rows))
            }
        }
        ContentLayout::GridCols { cols } => {
            let cols = cols as usize;
            if cols == 0 {
                (0, 0)
            } else {
                (n_children.div_ceil(cols), cols)
            }
        }
    }
}

/// Fills a `rows x cols` grid, row-major, from `children`. Children beyond
/// the grid's capacity (`rows * cols`) are not placed (dropped from flow
/// layout entirely).
fn build_grid(
    children: &[EntityId],
    content_layout: ContentLayout,
) -> (usize, usize, SmallVec<[Option<EntityId>; 16]>) {
    let (rows, cols) = grid_shape(content_layout, children.len());
    let capacity = rows.saturating_mul(cols);

    let mut cells: SmallVec<[Option<EntityId>; 16]> = smallvec![None; capacity];
    for (k, &child) in children.iter().enumerate().take(capacity) {
        cells[k] = Some(child);
    }

    (rows, cols, cells)
}

/// A flow child's resolved per-axis contribution (post `Relative` override)
/// plus its resolved alignment, computed once up front so the arrange pass
/// can both aggregate row/column `Line`s from it and, later, place it within
/// its granted slot without re-deriving anything.
struct CellInfo {
    entity: EntityId,
    row_line: Line,
    col_line: Line,
    align: Align2,
}

/// Builds the grid of [`CellInfo`] for a container's flow children, applying
/// the `Relative` override (`own_size` is this container's own already-known
/// size, per the core invariant) and resolving each cell's alignment
/// (its own `align` attribute, falling back to the parent's `content_align`).
fn build_cells(
    flow_children: &[EntityId],
    content_layout: ContentLayout,
    own_size: Size,
    parent_content_align: Align2,
    attrs_view: &View<'_, (&LayoutAttributes, &Measure, Option<&Container>)>,
    ui: &Ui,
) -> (usize, usize, SmallVec<[Option<CellInfo>; 16]>) {
    let (rows, cols, grid) = build_grid(flow_children, content_layout);

    let cells = grid
        .into_iter()
        .map(|slot| {
            slot.and_then(|child| {
                let (child_layout, child_measure, _) = attrs_view.try_get(child).ok()?;
                let child_margin = child_layout
                    .outer_margin
                    .unwrap_or_else(|| ui.default_outer_margin());

                let mut row_line = cell_line_row(*child_measure, child_margin);
                let mut col_line = cell_line_col(*child_measure, child_margin);

                if let Some(WidgetSize::Relative(rw, rh)) = child_layout.size {
                    row_line.preferred = apply_ratio(own_size.h, rh).max(row_line.min);
                    col_line.preferred = apply_ratio(own_size.w, rw).max(col_line.min);
                }

                let align = child_layout.align.unwrap_or(parent_content_align);

                Some(CellInfo {
                    entity: child,
                    row_line,
                    col_line,
                    align,
                })
            })
        })
        .collect();

    (rows, cols, cells)
}

/// Bottom-up measure pass for `id` and, recursively, its subtree. Writes the
/// computed [`Measure`] into `sizes` and also returns it (so a caller mid
/// recursion, i.e. a parent aggregating its own children's `Line`s, can use
/// it without a round-trip through the view).
fn measure_widget(
    id: EntityId,
    attrs_view: &View<'_, (&LayoutAttributes, Option<&Container>, Option<&Text>)>,
    sizes: &mut View<'_, &mut Measure>,
    ui: &Ui,
) -> Measure {
    let default_measure = Measure {
        min: Size::ZERO,
        preferred: Size::ZERO,
        stretches: Stretch::NONE,
    };

    let Ok((layout, container, text)) = attrs_view.try_get(id) else {
        return default_measure;
    };

    let widget_size = layout.size.unwrap_or(WidgetSize::Flexible(Stretch::NONE));

    let (content_min, content_pref) = if let Some(container) = container {
        let content_layout = layout
            .content_layout
            .unwrap_or_else(|| ui.default_content_layout());
        let inner_margin = layout
            .inner_margin
            .unwrap_or_else(|| ui.default_inner_margin());

        // Recurse into every child first (post-order), regardless of
        // Auto/Fixed position: every widget needs its own bottom-up
        // `Measure`, even ones that won't participate in this container's
        // own `Line` aggregation below.
        for &child in &container.children {
            measure_widget(child, attrs_view, sizes, ui);
        }

        // Only flow (Auto-positioned) children participate in `Line`
        // aggregation; absolutely-positioned children are handled entirely
        // separately at arrange time.
        let flow_children: SmallVec<[EntityId; 8]> = container
            .children
            .iter()
            .copied()
            .filter(|&child| {
                attrs_view
                    .try_get(child)
                    .map(|(child_layout, _, _)| child_layout.position.unwrap_or_default())
                    .unwrap_or_default()
                    == WidgetPos::Auto
            })
            .collect();

        let (rows, cols, cells) = build_grid(&flow_children, content_layout);

        let mut row_lines: SmallVec<[Line; 8]> = smallvec![EMPTY_LINE; rows];
        let mut col_lines: SmallVec<[Line; 8]> = smallvec![EMPTY_LINE; cols];

        for r in 0..rows {
            for c in 0..cols {
                let Some(child) = cells[r * cols + c] else {
                    continue;
                };
                let Some(child_measure) = sizes.try_get_mut(child).ok().map(|m| *m) else {
                    continue;
                };
                let Ok((child_layout, _, _)) = attrs_view.try_get(child) else {
                    continue;
                };
                let child_margin = child_layout
                    .outer_margin
                    .unwrap_or_else(|| ui.default_outer_margin());

                row_lines[r] = merge_line(row_lines[r], cell_line_row(child_measure, child_margin));
                col_lines[c] = merge_line(col_lines[c], cell_line_col(child_measure, child_margin));
            }
        }

        let (h_min, h_pref) = sum_min_pref(
            &row_lines,
            inner_margin.top as i32,
            inner_margin.bottom as i32,
        );
        let (w_min, w_pref) = sum_min_pref(
            &col_lines,
            inner_margin.left as i32,
            inner_margin.right as i32,
        );

        (
            Size { w: w_min, h: h_min },
            Size {
                w: w_pref,
                h: h_pref,
            },
        )
    } else if let Some(text) = text {
        let font_id = layout.font.unwrap_or_else(|| ui.default_font());
        let inner_margin = layout
            .inner_margin
            .unwrap_or_else(|| ui.default_inner_margin());
        let content_size = ui
            .font(font_id)
            .map(|f| f.text_bbox(&text.string).size())
            .unwrap_or(Size::ZERO);

        (content_size, content_size + inner_margin.size())
    } else {
        (Size::ZERO, Size::ZERO)
    };

    // A `Relative`-sized widget reports `preferred = min` at measure time: a
    // neutral, non-inflating value, since its real ratio-based size can't be
    // known bottom-up (no ancestor size exists yet during measure).
    let (min, preferred, stretches) = match widget_size {
        WidgetSize::Fixed(size) => (content_min, size, Stretch::NONE),
        WidgetSize::Relative(_, _) => (content_min, content_min, Stretch::NONE),
        WidgetSize::Flexible(stretch) => (content_min, content_pref, stretch),
    };

    let measure = Measure {
        min,
        preferred,
        stretches,
    };

    if let Ok(slot) = sizes.try_get_mut(id) {
        *slot = measure;
    }

    measure
}

/// Top-down arrange pass for `id`, given its own already-resolved `rect`
/// (per the core invariant). Writes `id`'s own [`Arranged`], then, if it's a
/// container, builds its row/column `Line`s from its direct flow children
/// (applying the `Relative` override), runs [`distribute_axis`] once per
/// axis, places each flow child within its granted slot, and recurses.
/// Absolutely-positioned children are placed afterward, offset from this
/// container's own resolved origin.
fn arrange_widget(
    id: EntityId,
    rect: Rect,
    layer: u32,
    attrs_view: &View<'_, (&LayoutAttributes, &Measure, Option<&Container>)>,
    arranged: &mut View<'_, &mut Arranged>,
    ui: &Ui,
) {
    if let Ok(slot) = arranged.try_get_mut(id) {
        *slot = Arranged { rect, layer };
    }

    let Ok((layout, measure, container)) = attrs_view.try_get(id) else {
        return;
    };
    let Some(container) = container else {
        // Leaf: nothing further to arrange.
        return;
    };

    let content_layout = layout
        .content_layout
        .unwrap_or_else(|| ui.default_content_layout());
    let content_align = layout
        .content_align
        .unwrap_or_else(|| ui.default_content_align());
    let inner_margin = layout
        .inner_margin
        .unwrap_or_else(|| ui.default_inner_margin());
    let own_stretches = measure.stretches;
    let own_size = rect.size();

    let mut flow_children: SmallVec<[EntityId; 8]> = SmallVec::new();
    let mut absolute_children: SmallVec<[EntityId; 4]> = SmallVec::new();
    for &child in &container.children {
        let Ok((child_layout, _, _)) = attrs_view.try_get(child) else {
            continue;
        };
        match child_layout.position.unwrap_or_default() {
            WidgetPos::Auto => flow_children.push(child),
            WidgetPos::Fixed { .. } => absolute_children.push(child),
        }
    }

    let (rows, cols, cells) = build_cells(
        &flow_children,
        content_layout,
        own_size,
        content_align,
        attrs_view,
        ui,
    );

    let mut row_lines: SmallVec<[Line; 8]> = smallvec![EMPTY_LINE; rows];
    let mut col_lines: SmallVec<[Line; 8]> = smallvec![EMPTY_LINE; cols];
    for r in 0..rows {
        for c in 0..cols {
            if let Some(cell) = &cells[r * cols + c] {
                row_lines[r] = merge_line(row_lines[r], cell.row_line);
                col_lines[c] = merge_line(col_lines[c], cell.col_line);
            }
        }
    }

    let row_layout = distribute_axis(
        &row_lines,
        inner_margin.top as i32,
        inner_margin.bottom as i32,
        own_stretches.vertical,
        own_size.h,
    );
    let col_layout = distribute_axis(
        &col_lines,
        inner_margin.left as i32,
        inner_margin.right as i32,
        own_stretches.horizontal,
        own_size.w,
    );

    let row_offsets = axis_offsets(&row_layout);
    let col_offsets = axis_offsets(&col_layout);

    for r in 0..rows {
        for c in 0..cols {
            let Some(cell) = &cells[r * cols + c] else {
                continue;
            };
            let slot = Rect::from_pos_size(
                Pos {
                    x: rect.lt.x + col_offsets[c],
                    y: rect.lt.y + row_offsets[r],
                },
                Size {
                    w: col_layout.item_sizes[c],
                    h: row_layout.item_sizes[r],
                },
            );
            let child_rect = place_in_slot(cell.row_line, cell.col_line, slot, cell.align);
            arrange_widget(cell.entity, child_rect, layer + 1, attrs_view, arranged, ui);
        }
    }

    for &child in &absolute_children {
        let Ok((child_layout, child_measure, _)) = attrs_view.try_get(child) else {
            continue;
        };
        let WidgetPos::Fixed { offset, anchor } = child_layout.position.unwrap_or_default() else {
            continue;
        };
        let size = resolve_absolute_size(child_layout, *child_measure, own_size);
        let base = Align2::from(anchor).in_rect(rect, size);
        let child_rect = Rect::from_pos_size(base.lt + offset, size);
        arrange_widget(child, child_rect, layer + 1, attrs_view, arranged, ui);
    }
}

/// Resolves and arranges a root widget: the seed of the top-down pass. Per
/// the core invariant, root is not a special case beyond being the seed —
/// it is resolved as the single cell of a virtual 1x1 grid whose available
/// space is `ui.rect()`. A `Fixed`-position root instead behaves like an
/// absolutely-positioned child of that same virtual container: offset from
/// `ui.rect()`'s own origin, sized via the normal (non-slot) resolution
/// rule, bypassing alignment entirely.
fn arrange_root(
    id: EntityId,
    attrs_view: &View<'_, (&LayoutAttributes, &Measure, Option<&Container>)>,
    arranged: &mut View<'_, &mut Arranged>,
    ui: &Ui,
) {
    let Ok((layout, measure, _)) = attrs_view.try_get(id) else {
        return;
    };

    let ui_rect = ui.rect();
    let own_available = ui_rect.size();

    match layout.position.unwrap_or_default() {
        WidgetPos::Fixed { offset, anchor } => {
            let size = resolve_absolute_size(layout, *measure, own_available);
            let base = Align2::from(anchor).in_rect(ui_rect, size);
            let rect = Rect::from_pos_size(base.lt + offset, size);
            arrange_widget(id, rect, 0, attrs_view, arranged, ui);
        }
        WidgetPos::Auto => {
            let (pref_w, pref_h, stretch_w, stretch_h) = match layout.size {
                Some(WidgetSize::Relative(rw, rh)) => (
                    apply_ratio(own_available.w, rw).max(measure.min.w),
                    apply_ratio(own_available.h, rh).max(measure.min.h),
                    false,
                    false,
                ),
                _ => (
                    measure.preferred.w,
                    measure.preferred.h,
                    measure.stretches.horizontal,
                    measure.stretches.vertical,
                ),
            };

            let size = Size {
                w: resolve_size_component(pref_w, stretch_w, own_available.w),
                h: resolve_size_component(pref_h, stretch_h, own_available.h),
            };

            let align = layout.align.unwrap_or_else(|| ui.default_content_align());
            let rect = align.in_rect(ui_rect, size);

            arrange_widget(id, rect, 0, attrs_view, arranged, ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{font::FontId, style::Anchor};

    // ------------------------------------------------------------------
    // White-box tests of the axis-distribution algorithm and its helpers.
    // ------------------------------------------------------------------

    fn line(min: i32, preferred: i32, stretch: bool) -> Line {
        Line {
            min,
            preferred,
            stretch,
            margin_leading: 0,
            margin_trailing: 0,
        }
    }

    #[test]
    fn regime_boundary_size_equals_preferred() {
        let items = [line(10, 30, false), line(5, 20, false)];
        let result = distribute_axis(&items, 0, 0, false, 50);
        assert_eq!(&result.item_sizes[..], &[30, 20]);
        assert_eq!(&result.gaps[..], &[0, 0, 0]);
    }

    #[test]
    fn regime_boundary_size_equals_min() {
        let items = [line(10, 30, false), line(5, 20, false)];
        let result = distribute_axis(&items, 0, 0, false, 15);
        assert_eq!(&result.item_sizes[..], &[10, 5]);
        assert_eq!(&result.gaps[..], &[0, 0, 0]);
    }

    #[test]
    fn regime1_stretch_item_absorbs_surplus() {
        let items = [line(10, 30, true), line(5, 20, false)];
        let result = distribute_axis(&items, 0, 0, false, 80);
        // S_pref = 50, surplus = 30, all routed to the sole stretching item.
        assert_eq!(&result.item_sizes[..], &[60, 20]);
    }

    #[test]
    fn regime1_no_consumer_leaves_surplus_unused() {
        let items = [line(10, 30, false), line(5, 20, false)];
        let result = distribute_axis(&items, 0, 0, false, 1000);
        // No item stretches and the container doesn't either: content stays
        // at its preferred size, the rest of `size` is simply not consumed
        // by this axis call (left for the caller/align step to handle).
        assert_eq!(&result.item_sizes[..], &[30, 20]);
    }

    #[test]
    fn regime3_deep_shrink_levels_rather_than_scales_proportionally() {
        let items = [
            line(5, 5, false),
            line(50, 50, false),
            line(100, 100, false),
        ];
        let result = distribute_axis(&items, 0, 0, false, 40);

        // Smallest cap (5) is small enough to be granted in full immediately.
        assert_eq!(result.item_sizes[0], 5);

        // The remaining budget (35) is leveled across the two bigger items
        // (50 and 100): leveling caps them at ~17-18 each, not a
        // proportional 50:100 split (which would give roughly 11.7 vs 23.3).
        let a = result.item_sizes[1];
        let b = result.item_sizes[2];
        assert!(
            (a - b).abs() <= 1,
            "expected near-equal leveling, got {a} vs {b}"
        );
        assert_eq!(a + b, 35);

        // Neither one is anywhere near the proportional split.
        assert!(a < 25 && b < 25);
    }

    #[test]
    fn cross_axis_largest_margin_gives_way_first_under_extreme_shortage() {
        // A single item with no content, but a huge leading inset (100) and
        // a modest trailing inset (10) — exactly the "cross-axis" shape:
        // the pool is [item's own pref-min headroom, leading gap, trailing
        // gap], and under shortage the largest demand (the leading gap)
        // should give way first, while the smaller trailing gap survives
        // intact.
        let items = [line(0, 0, false)];
        let result = distribute_axis(&items, 100, 10, false, 50);

        assert_eq!(result.item_sizes[0], 0);
        assert_eq!(result.gaps[1], 10, "smaller demand preserved in full");
        assert_eq!(
            result.gaps[0], 40,
            "larger demand clamped down to the water level"
        );
    }

    #[test]
    fn water_fill_shrink_basic_leveling() {
        let caps = [5, 50, 100];
        let grants = water_fill_shrink(&caps, 40);
        assert_eq!(grants[0], 5);
        assert_eq!(grants[1] + grants[2], 35);
        assert!((grants[1] - grants[2]).abs() <= 1);
    }

    #[test]
    fn water_fill_shrink_budget_covers_everything() {
        let caps = [1, 2, 3];
        let grants = water_fill_shrink(&caps, 6);
        assert_eq!(&grants[..], &[1, 2, 3]);
    }

    #[test]
    fn distribute_remainder_sums_exactly() {
        let shares = distribute_remainder(10, 3);
        assert_eq!(shares.iter().sum::<i32>(), 10);
        assert_eq!(&shares[..], &[4, 3, 3]);
    }

    // ------------------------------------------------------------------
    // Integration tests: build a small world, run `layout_system`, inspect
    // the resulting `Arranged` rects.
    // ------------------------------------------------------------------

    fn new_ui_world(w: i32, h: i32) -> World {
        let mut world = World::new();
        let mut ui = Ui::new();
        ui.set_rect(Rect::from_pos_size(Pos::ZERO, Size { w, h }));
        world.insert_resource(ui);
        world
    }

    fn leaf(world: &mut World, attrs: LayoutAttributes) -> EntityId {
        world.spawn((Widget { parent: None }, attrs)).id()
    }

    fn container(
        world: &mut World,
        attrs: LayoutAttributes,
        children: std::vec::Vec<EntityId>,
    ) -> EntityId {
        world
            .spawn((Widget { parent: None }, attrs, Container { children }))
            .id()
    }

    fn root_leaf(world: &mut World, attrs: LayoutAttributes) -> EntityId {
        world
            .spawn((Widget { parent: None }, RootWidget, attrs))
            .id()
    }

    fn root_container(
        world: &mut World,
        attrs: LayoutAttributes,
        children: std::vec::Vec<EntityId>,
    ) -> EntityId {
        world
            .spawn((
                Widget { parent: None },
                RootWidget,
                attrs,
                Container { children },
            ))
            .id()
    }

    fn fixed(w: i32, h: i32) -> LayoutAttributes {
        LayoutAttributes {
            size: Some(WidgetSize::Fixed(Size { w, h })),
            ..Default::default()
        }
    }

    fn rect_of(world: &mut World, id: EntityId) -> Rect {
        world.get::<&Arranged>(id).unwrap().rect
    }

    #[test]
    fn relative_resolves_via_immediate_parent_at_every_depth() {
        let mut world = new_ui_world(300, 300);

        // root: Fixed 200x100.
        // a: Relative(1/2, 1/2) of root -> 100x50.
        // b: Relative(1/2, 1/2) of a (NOT of root) -> 50x25.
        let half = Ratio::new(1, NonZero::new(2).unwrap());

        let b = leaf(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Relative(half, half)),
                ..Default::default()
            },
        );
        let a = container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Relative(half, half)),
                ..Default::default()
            },
            vec![b],
        );
        let root = root_container(&mut world, fixed(200, 100), vec![a]);

        layout_system(&mut world);

        assert_eq!(rect_of(&mut world, root).size(), Size { w: 200, h: 100 });
        assert_eq!(rect_of(&mut world, a).size(), Size { w: 100, h: 50 });
        assert_eq!(rect_of(&mut world, b).size(), Size { w: 50, h: 25 });
    }

    #[test]
    fn vertical_stack_shares_cross_axis_inset_across_the_whole_column() {
        let mut world = new_ui_world(400, 400);

        let child_small_margin = leaf(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 10, h: 10 })),
                outer_margin: Some(Margin {
                    left: 5,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }),
                ..Default::default()
            },
        );
        let child_big_margin = leaf(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 100, h: 10 })),
                outer_margin: Some(Margin {
                    left: 20,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }),
                ..Default::default()
            },
        );

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 200, h: 200 })),
                content_layout: Some(ContentLayout::VerticalStack),
                ..Default::default()
            },
            vec![child_small_margin, child_big_margin],
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let small_rect = rect_of(&mut world, child_small_margin);
        let big_rect = rect_of(&mut world, child_big_margin);

        // Both rows share the SAME column, so both get inset by the MAX
        // left margin (20) across the whole column, not their own smaller
        // individual margin (5, in `child_small_margin`'s case).
        assert_eq!(small_rect.lt.x, root_rect.lt.x + 20);
        assert_eq!(big_rect.lt.x, root_rect.lt.x + 20);
    }

    #[test]
    fn horizontal_stack_basic_side_by_side() {
        let mut world = new_ui_world(300, 100);

        let a = leaf(&mut world, fixed(30, 20));
        let b = leaf(&mut world, fixed(40, 20));

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 200, h: 50 })),
                content_layout: Some(ContentLayout::HorizontalStack),
                ..Default::default()
            },
            vec![a, b],
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let a_rect = rect_of(&mut world, a);
        let b_rect = rect_of(&mut world, b);

        assert_eq!(a_rect.lt.x, root_rect.lt.x);
        assert_eq!(a_rect.size(), Size { w: 30, h: 20 });
        assert_eq!(b_rect.lt.x, a_rect.rb.x);
        assert_eq!(b_rect.size(), Size { w: 40, h: 20 });
    }

    #[test]
    fn grid_basic_two_by_two() {
        let mut world = new_ui_world(300, 300);

        let children: std::vec::Vec<EntityId> =
            (0..4).map(|_| leaf(&mut world, fixed(20, 10))).collect();

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 100, h: 60 })),
                content_layout: Some(ContentLayout::Grid { rows: 2, cols: 2 }),
                ..Default::default()
            },
            children.clone(),
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let r0c0 = rect_of(&mut world, children[0]);
        let r0c1 = rect_of(&mut world, children[1]);
        let r1c0 = rect_of(&mut world, children[2]);

        assert_eq!(r0c0.lt, root_rect.lt);
        assert_eq!(r0c1.lt.x, r0c0.rb.x);
        assert_eq!(r0c1.lt.y, r0c0.lt.y);
        assert_eq!(r1c0.lt.y, r0c0.rb.y);
        assert_eq!(r1c0.lt.x, r0c0.lt.x);
    }

    #[test]
    fn grid_shape_rows_cols_derive_via_ceiling_division() {
        // 5 children, 2 fixed rows -> cols = ceil(5/2) = 3.
        assert_eq!(grid_shape(ContentLayout::GridRows { rows: 2 }, 5), (2, 3));
        // 5 children, 2 fixed cols -> rows = ceil(5/2) = 3.
        assert_eq!(grid_shape(ContentLayout::GridCols { cols: 2 }, 5), (3, 2));
        // Exact multiples don't round up.
        assert_eq!(grid_shape(ContentLayout::GridRows { rows: 2 }, 6), (2, 3));
        assert_eq!(grid_shape(ContentLayout::GridCols { cols: 3 }, 6), (2, 3));
        // No children -> the other dimension is 0, not a division panic.
        assert_eq!(grid_shape(ContentLayout::GridRows { rows: 4 }, 0), (4, 0));
        // A zero fixed dimension degenerates to an empty grid rather than
        // dividing by zero.
        assert_eq!(grid_shape(ContentLayout::GridRows { rows: 0 }, 5), (0, 0));
        assert_eq!(grid_shape(ContentLayout::GridCols { cols: 0 }, 5), (0, 0));
    }

    #[test]
    fn grid_rows_lays_out_row_major_with_derived_column_count() {
        let mut world = new_ui_world(300, 300);

        // 5 children, 2 rows -> derived cols = ceil(5/2) = 3, row-major fill:
        // row0 = [0,1,2], row1 = [3,4] (last cell empty).
        let children: std::vec::Vec<EntityId> =
            (0..5).map(|_| leaf(&mut world, fixed(20, 10))).collect();

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 100, h: 60 })),
                content_layout: Some(ContentLayout::GridRows { rows: 2 }),
                ..Default::default()
            },
            children.clone(),
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let r0c0 = rect_of(&mut world, children[0]);
        let r0c1 = rect_of(&mut world, children[1]);
        let r0c2 = rect_of(&mut world, children[2]);
        let r1c0 = rect_of(&mut world, children[3]);

        assert_eq!(r0c0.lt, root_rect.lt);
        assert_eq!(r0c1.lt.x, r0c0.rb.x);
        assert_eq!(r0c1.lt.y, r0c0.lt.y);
        assert_eq!(r0c2.lt.x, r0c1.rb.x);
        assert_eq!(r1c0.lt.y, r0c0.rb.y);
        assert_eq!(r1c0.lt.x, r0c0.lt.x);
    }

    #[test]
    fn grid_cols_lays_out_row_major_with_derived_row_count() {
        let mut world = new_ui_world(300, 300);

        // 5 children, 2 cols -> derived rows = ceil(5/2) = 3, row-major fill:
        // row0 = [0,1], row1 = [2,3], row2 = [4] (second cell empty).
        let children: std::vec::Vec<EntityId> =
            (0..5).map(|_| leaf(&mut world, fixed(20, 10))).collect();

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 60, h: 90 })),
                content_layout: Some(ContentLayout::GridCols { cols: 2 }),
                ..Default::default()
            },
            children.clone(),
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let r0c0 = rect_of(&mut world, children[0]);
        let r0c1 = rect_of(&mut world, children[1]);
        let r1c0 = rect_of(&mut world, children[2]);
        let r2c0 = rect_of(&mut world, children[4]);

        assert_eq!(r0c0.lt, root_rect.lt);
        assert_eq!(r0c1.lt.x, r0c0.rb.x);
        assert_eq!(r0c1.lt.y, r0c0.lt.y);
        assert_eq!(r1c0.lt.y, r0c0.rb.y);
        assert_eq!(r1c0.lt.x, r0c0.lt.x);
        assert_eq!(r2c0.lt.y, r1c0.rb.y);
    }

    #[test]
    fn nested_containers_resolve_bottom_up_and_top_down() {
        let mut world = new_ui_world(300, 300);

        let leaf_a = leaf(&mut world, fixed(20, 20));
        let leaf_b = leaf(&mut world, fixed(20, 20));
        let inner = container(
            &mut world,
            LayoutAttributes {
                content_layout: Some(ContentLayout::HorizontalStack),
                ..Default::default()
            },
            vec![leaf_a, leaf_b],
        );

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 200, h: 100 })),
                ..Default::default()
            },
            vec![inner],
        );

        layout_system(&mut world);

        let inner_rect = rect_of(&mut world, inner);
        // inner shrink-to-fits its two 20x20 children laid out horizontally.
        assert_eq!(inner_rect.size(), Size { w: 40, h: 20 });

        let a_rect = rect_of(&mut world, leaf_a);
        let b_rect = rect_of(&mut world, leaf_b);
        assert_eq!(a_rect.lt, inner_rect.lt);
        assert_eq!(b_rect.lt.x, a_rect.rb.x);

        let _ = root;
    }

    #[test]
    fn absolute_child_ignores_flow_and_uses_own_offset() {
        let mut world = new_ui_world(300, 300);

        let flow_child = leaf(&mut world, fixed(20, 20));
        let absolute_child = leaf(
            &mut world,
            LayoutAttributes {
                position: Some(WidgetPos::Fixed {
                    offset: Vec { x: 5, y: 7 },
                    anchor: Anchor::TopLeft,
                }),
                size: Some(WidgetSize::Fixed(Size { w: 15, h: 15 })),
                ..Default::default()
            },
        );

        let root = root_container(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Fixed(Size { w: 200, h: 100 })),
                ..Default::default()
            },
            vec![flow_child, absolute_child],
        );

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        let abs_rect = rect_of(&mut world, absolute_child);

        assert_eq!(
            abs_rect,
            Rect::from_pos_size(
                Pos {
                    x: root_rect.lt.x + 5,
                    y: root_rect.lt.y + 7,
                },
                Size { w: 15, h: 15 },
            )
        );
    }

    #[test]
    fn root_fixed_size_is_exact_and_clamped_to_ui_rect_when_larger() {
        let mut world = new_ui_world(300, 300);
        let root = root_leaf(&mut world, fixed(100, 50));

        layout_system(&mut world);

        assert_eq!(rect_of(&mut world, root).size(), Size { w: 100, h: 50 });
    }

    #[test]
    fn root_relative_size_uses_ui_rect_as_basis() {
        let mut world = new_ui_world(200, 90);
        let third = Ratio::new(1, NonZero::new(3).unwrap());
        let half = Ratio::new(1, NonZero::new(2).unwrap());

        let root = root_leaf(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Relative(half, third)),
                ..Default::default()
            },
        );

        layout_system(&mut world);

        assert_eq!(rect_of(&mut world, root).size(), Size { w: 100, h: 30 });
    }

    #[test]
    fn root_shrink_to_fit_does_not_fill_ui_rect() {
        let mut world = new_ui_world(300, 300);

        let child = leaf(&mut world, fixed(50, 30));
        let root = root_container(&mut world, LayoutAttributes::default(), vec![child]);

        layout_system(&mut world);

        let root_rect = rect_of(&mut world, root);
        assert_eq!(root_rect.size(), Size { w: 50, h: 30 });
        assert_eq!(root_rect.lt, Pos::ZERO);
    }

    #[test]
    fn root_stretch_fills_ui_rect() {
        let mut world = new_ui_world(150, 80);

        let root = root_leaf(
            &mut world,
            LayoutAttributes {
                size: Some(WidgetSize::Flexible(Stretch::BOTH)),
                ..Default::default()
            },
        );

        layout_system(&mut world);

        assert_eq!(rect_of(&mut world, root).size(), Size { w: 150, h: 80 });
    }

    #[test]
    fn text_leaf_measures_via_font_bbox() {
        let mut world = new_ui_world(300, 300);
        let text_id = world
            .spawn((
                Widget { parent: None },
                RootWidget,
                LayoutAttributes::default(),
                Text::new("Hi".to_string()),
            ))
            .id();

        layout_system(&mut world);

        let rect = rect_of(&mut world, text_id);
        // Non-empty text against a default (non-fixed, non-stretching) size
        // should shrink-to-fit to a non-zero content size.
        assert!(rect.size().w > 0);
        assert!(rect.size().h > 0);

        let _ = FontId(0);
    }
}
