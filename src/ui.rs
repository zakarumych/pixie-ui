use std::{borrow::Cow, convert::Infallible, num::NonZeroU64};

use edict::{entity::EntityId, query::Entities, world::World};
use foldhash::fast::RandomState;
use hashbrown::HashMap;

use crate::{
    align::{Align, Align2},
    color::Color,
    draw::{Brush, Draw, Stroke},
    event::{Key, PixieEvent},
    focus::{FocusOnClick, collect_focus_cycle_order},
    font::{Font, FontId},
    layout::{Arranged, ContentLayout},
    margin::Margin,
    math::{Pos, Rect},
    style::{InputState, ResolvedAttributes},
    text::{Glyph, Text},
    texture::TextureId,
    trigger::OnClick,
    widget::{Container, SensesClicks, SensesCursor, Widget},
};

/// A user-interface resource.
pub struct Ui {
    fonts: HashMap<FontId, Font, RandomState>,
    next_texture_id: NonZeroU64,
    default_content_layout: ContentLayout,
    default_content_align: Align2,
    default_outer_margin: Margin,
    default_inner_margin: Margin,
    default_fg_color: Color,
    default_bg_color: Color,
    default_text_font: FontId,
    default_text_color: Color,
    rect: Rect,
    input: InputState,
    cycle_focus_key: Option<Key>,
}

impl Ui {
    /// Creates a new user-interface resource.
    pub fn new() -> Self {
        let var5x7 = FontId(0);
        let mono5x7 = FontId(1);

        let mut fonts = HashMap::default();
        fonts.insert(var5x7, crate::font::var5x7());
        fonts.insert(mono5x7, crate::font::mono5x7());

        Ui {
            fonts,
            next_texture_id: const { NonZeroU64::new(1).unwrap() },
            default_content_layout: ContentLayout::VerticalStack,
            default_content_align: Align2::from(Align::Start),
            default_outer_margin: Margin::ZERO,
            default_inner_margin: Margin::ZERO,
            default_fg_color: Color::WHITE,
            default_bg_color: Color::BLACK,
            default_text_font: FontId(0),
            default_text_color: Color::WHITE,
            rect: Rect::ZERO,
            input: InputState::default(),
            cycle_focus_key: Some(Key::Tab),
        }
    }

    /// Registers a font and returns a unique identifier for it.
    pub fn register_font(&mut self, font: Font) -> FontId {
        let font_id = FontId(self.fonts.len() as u32);
        self.fonts.insert(font_id, font);
        font_id
    }

    /// Retrieves a font by its unique identifier.
    pub fn font(&self, font_id: FontId) -> Option<&Font> {
        self.fonts.get(&font_id)
    }

    /// Generates a new unique identifier for a texture resource.
    pub fn new_texture_id(&mut self) -> TextureId {
        let texture_id = TextureId(self.next_texture_id);
        self.next_texture_id = self.next_texture_id.saturating_add(1);
        texture_id
    }

    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn default_content_layout(&self) -> ContentLayout {
        self.default_content_layout
    }

    pub fn default_content_align(&self) -> Align2 {
        self.default_content_align
    }

    pub fn default_outer_margin(&self) -> Margin {
        self.default_outer_margin
    }

    pub fn default_inner_margin(&self) -> Margin {
        self.default_inner_margin
    }

    pub fn default_fg_color(&self) -> Color {
        self.default_fg_color
    }

    pub fn default_bg_color(&self) -> Color {
        self.default_bg_color
    }

    pub fn default_text_font(&self) -> FontId {
        self.default_text_font
    }

    pub fn default_text_color(&self) -> Color {
        self.default_text_color
    }

    pub fn input(&self) -> &InputState {
        &self.input
    }

    pub fn set_focus(&mut self, id: EntityId) {
        self.input.focused = Some(id);
    }

    pub fn focused(&self) -> Option<EntityId> {
        self.input.focused
    }

    pub fn cycle_focus_key(&self) -> Option<Key> {
        self.cycle_focus_key
    }

    pub fn set_cycle_focus_key(&mut self, key: Option<Key>) {
        self.cycle_focus_key = key;
    }

    /// Walks every [`Widget`] entity and emits [`Draw`] commands describing how to render it,
    /// in painter's-algorithm order (a parent's own background is emitted before its children's).
    ///
    /// # Preconditions
    ///
    /// Every `Widget` entity that should be drawn must already have a [`Arranged`] component
    /// (i.e. this must run after [`crate::layout::resolve_rects`]) and a [`FinalAttributes`]
    /// component with theme/fallback merging applied (i.e. this must run after
    /// [`crate::style::Style::resolve_attributes`]). Widgets without a `Arranged` (and, by
    /// construction from `resolve_rects`, their entire subtree) are silently skipped.
    pub fn draw_ui<'a>(world: &mut World, commands: &mut impl Extend<Draw<'a>>) {
        let Some(ui) = world.remove_resource::<Ui>() else {
            return;
        };

        let roots: Vec<EntityId> = world
            .view::<(Entities, &Widget)>()
            .into_iter()
            .filter(|(_, w)| w.parent.is_none())
            .map(|(e, _)| e.id())
            .collect();

        let root_inherited = InheritedPaint {
            fg_brush: Brush::Solid(ui.default_fg_color),
            bg_brush: Brush::Solid(ui.default_bg_color),
            stroke: None,
            text_font: ui.default_text_font,
            text_brush: Brush::Solid(ui.default_text_color),
        };

        for root in roots {
            draw_widget(world, &ui, root, root_inherited, commands);
        }

        world.insert_resource(ui);
    }
}

/// Per-widget cascade of the paint-related attributes that inherit down the widget tree
/// (analogous to CSS inherited properties): a widget's own [`crate::style::Attributes`] field,
/// if `None`, falls back to its parent's already-resolved value for that field. Root widgets
/// fall back to [`Ui`]'s corresponding `default_*` value, as if `Ui` were their "parent".
#[derive(Clone, Copy)]
struct InheritedPaint {
    fg_brush: Brush,
    bg_brush: Brush,
    stroke: Option<Stroke>,
    text_font: FontId,
    text_brush: Brush,
}

/// Draws `id` and, recursively, its subtree, given the [`InheritedPaint`] cascaded down from
/// its parent (or from `Ui`'s defaults, for a root widget).
///
/// `world`'s borrow (`'w`) is deliberately a separate lifetime from the `Draw` data (`'a`):
/// nothing drawn here ever borrows from `world` (all emitted `Draw` values own their data, e.g.
/// `Cow::Owned` glyphs), so tying the two lifetimes together would force `world` to be borrowed
/// for all of `'a` at every call site, making recursion/looping impossible.
fn draw_widget<'w, 'a>(
    world: &'w mut World,
    ui: &Ui,
    id: EntityId,
    inherited: InheritedPaint,
    commands: &mut impl Extend<Draw<'a>>,
) {
    let Ok(rect) = world.get::<&Arranged>(id).map(|r| r.rect) else {
        // Unresolved widget: by construction from `resolve_rects`, its whole subtree is
        // also unresolved, so there's nothing further to draw here.
        return;
    };

    let Ok(attrs) = world.get::<&ResolvedAttributes>(id).map(|a| a.0) else {
        return;
    };

    let resolved = InheritedPaint {
        fg_brush: attrs.fg_brush.unwrap_or(inherited.fg_brush),
        bg_brush: attrs.bg_brush.unwrap_or(inherited.bg_brush),
        stroke: attrs.stroke.or(inherited.stroke),
        text_font: attrs.text_font.unwrap_or(inherited.text_font),
        text_brush: attrs.text_brush.unwrap_or(inherited.text_brush),
    };

    commands.extend(std::iter::once(Draw::Rect {
        geometry: rect,
        fill: Some(resolved.bg_brush),
        stroke: resolved.stroke,
    }));

    if let Ok(Some(text)) = world.get::<Option<&Text>>(id)
        && let Some(font) = ui.font(resolved.text_font)
    {
        let glyphs: Vec<Glyph> = text
            .string
            .chars()
            .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
            .collect();

        let text_align = attrs.text_align.unwrap_or(Align::Start.into());
        let text_rect = font.text_bbox(&text.string);

        let text_offset = text_align.rect_offset(rect, text_rect);

        commands.extend(std::iter::once(Draw::Text {
            start: text_offset,
            font: resolved.text_font,
            glyphs: Cow::Owned(glyphs),
            brush: resolved.text_brush,
        }));
    }

    let children = world
        .get::<Option<&Container>>(id)
        .ok()
        .flatten()
        .map(|c| c.children.clone());

    if let Some(children) = children {
        for child in children {
            draw_widget(world, ui, child, resolved, commands);
        }
    }
}

/// Feeds a [`PixieEvent`] into the UI, updating hover/press state by hit-testing
/// against widgets' [`Arranged`] rects (top-most/deepest layer wins on overlap —
/// z-fighting among widgets at the same layer is not resolved here). Hover
/// requires [`crate::widget::SensesCursor`]; press/release requires
/// [`crate::widget::SensesClicks`].
///
/// Pressing the [`Ui::cycle_focus_key`] (if set) advances focus to the next
/// [`FocusCycle`]-marked widget in tree order, wrapping to the first after the last.
/// Releasing a click on a [`FocusOnClick`]-marked widget focuses it.
///
/// Returns every action emitted in response to this event — currently just a
/// completed click (press and release both landing on the same widget), read
/// off that widget's [`crate::action::OnClick<A>`] component, if present.
pub fn handle_event<A: 'static>(
    world: &mut World,
    event: PixieEvent,
) -> impl Iterator<Item = A> + use<A> {
    let mut actions = smallvec::SmallVec::<[A; 1]>::new();

    let Some(ui) = world.get_resource_mut::<Ui>() else {
        return actions.into_iter();
    };

    let mut input = ui.input;
    let cycle_focus_key = ui.cycle_focus_key;
    drop(ui);

    match event {
        PixieEvent::CursorMoved { pos } => {
            input.cursor = Some(pos);
            input.hovered = hit_test::<SensesCursor>(world, pos);
        }
        PixieEvent::ButtonPressed => {
            if let Some(pos) = input.cursor {
                input.pressed = hit_test::<SensesClicks>(world, pos);
            }
        }
        PixieEvent::ButtonReleased => {
            if let Some(id) = input.pressed
                && input.hovered == Some(id)
            {
                if world.get::<&FocusOnClick>(id).is_ok() {
                    input.focused = Some(id);
                }

                if let Ok(mut on_click) = world.try_view_one::<&mut OnClick<Infallible>>(id) {
                    if let Some(on_click) = on_click.get_mut() {
                        on_click.invoke(world, id);
                    }
                }

                if core::any::TypeId::of::<A>() != core::any::TypeId::of::<Infallible>() {
                    if let Ok(mut on_click) = world.try_view_one::<&mut OnClick<A>>(id) {
                        if let Some(on_click) = on_click.get_mut() {
                            actions.extend(on_click.invoke(world, id));
                        }
                    }
                }
            }
            input.pressed = None;
        }
        PixieEvent::KeyPressed(key) => {
            if Some(key) == cycle_focus_key {
                let order = collect_focus_cycle_order(world);
                if !order.is_empty() {
                    input.focused = match input
                        .focused
                        .and_then(|id| order.iter().position(|&e| e == id))
                    {
                        Some(index) => Some(order[(index + 1) % order.len()]),
                        None => Some(order[0]),
                    };
                }
            }
        }
    }

    if let Some(mut ui) = world.get_resource_mut::<Ui>() {
        ui.input = input;
    }

    actions.into_iter()
}

/// Returns the entity with the deepest `Arranged.layer` (i.e. visually top-most)
/// among all `M`-marked widgets whose `Arranged.rect` contains `pos`, or `None`.
fn hit_test<M: edict::component::Component>(world: &World, pos: Pos) -> Option<EntityId> {
    let mut best: Option<(EntityId, u32)> = None;
    for (e, arranged) in world.view::<(Entities, &Arranged)>().with::<M>() {
        if arranged.rect.contains(pos) && best.is_none_or(|(_, layer)| arranged.layer >= layer) {
            best = Some((e.id(), arranged.layer));
        }
    }
    best.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;
    use crate::{
        event::Key,
        focus::FocusCycle,
        math::Size,
        widget::{SensesClicks, SensesCursor},
    };

    fn spawn_focus_cycle_widget(world: &mut World) -> EntityId {
        world.spawn((Widget { parent: None }, FocusCycle)).id()
    }

    fn spawn_clickable(world: &mut World, focus_on_click: bool, rect: Rect) -> EntityId {
        let id = world
            .spawn((
                Widget { parent: None },
                SensesCursor,
                SensesClicks,
                Arranged { rect, layer: 0 },
            ))
            .id();
        if focus_on_click {
            world.insert(id, FocusOnClick).unwrap();
        }
        id
    }

    #[test]
    fn tab_cycle_wraps_from_last_to_first() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let first = spawn_focus_cycle_widget(&mut world);
        let last = spawn_focus_cycle_widget(&mut world);

        world.get_resource_mut::<Ui>().unwrap().set_focus(last);
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::KeyPressed(Key::Tab)).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), Some(first));
    }

    #[test]
    fn tab_cycle_with_nothing_focused_picks_first() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let first = spawn_focus_cycle_widget(&mut world);
        let _second = spawn_focus_cycle_widget(&mut world);

        let _ = handle_event::<Infallible>(&mut world, PixieEvent::KeyPressed(Key::Tab)).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), Some(first));
    }

    #[test]
    fn cycle_focus_key_none_disables_tab_cycling() {
        let mut world = World::new();
        let mut ui = Ui::new();
        ui.set_cycle_focus_key(None);
        world.insert_resource(ui);

        let _first = spawn_focus_cycle_widget(&mut world);
        let _second = spawn_focus_cycle_widget(&mut world);

        let _ = handle_event::<Infallible>(&mut world, PixieEvent::KeyPressed(Key::Tab)).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), None);
    }

    #[test]
    fn button_released_focuses_focus_on_click_widget() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let rect = Rect::from_pos_size(Pos::ZERO, Size { w: 10, h: 10 });
        let id = spawn_clickable(&mut world, true, rect);

        let pos = Pos { x: 5, y: 5 };
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::CursorMoved { pos }).count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonPressed).count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonReleased).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), Some(id));
    }

    #[test]
    fn button_released_ignores_widget_without_focus_on_click_marker() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let rect = Rect::from_pos_size(Pos::ZERO, Size { w: 10, h: 10 });
        let _id = spawn_clickable(&mut world, false, rect);

        let pos = Pos { x: 5, y: 5 };
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::CursorMoved { pos }).count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonPressed).count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonReleased).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), None);
    }

    #[test]
    fn button_released_ignores_mismatched_press_hover() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let rect_a = Rect::from_pos_size(Pos::ZERO, Size { w: 10, h: 10 });
        let rect_b = Rect::from_pos_size(Pos { x: 20, y: 20 }, Size { w: 10, h: 10 });
        let _a = spawn_clickable(&mut world, true, rect_a);
        let _b = spawn_clickable(&mut world, true, rect_b);

        // Press on `a`, then move the cursor to hover over `b` before releasing.
        let _ = handle_event::<Infallible>(
            &mut world,
            PixieEvent::CursorMoved { pos: Pos { x: 5, y: 5 } },
        )
        .count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonPressed).count();
        let _ = handle_event::<Infallible>(
            &mut world,
            PixieEvent::CursorMoved {
                pos: Pos { x: 25, y: 25 },
            },
        )
        .count();
        let _ = handle_event::<Infallible>(&mut world, PixieEvent::ButtonReleased).count();

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), None);
    }
}
