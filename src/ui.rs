use std::{borrow::Cow, num::NonZeroU64, time::Instant};

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
    layout::{Arranged, ContentLayout, shrink_cross_inset},
    margin::Margin,
    math::{Pos, Rect, Size},
    style::{InputState, ResolvedAttributes},
    text::{Glyph, Text, TextInput},
    texture::TextureId,
    trigger::{NoAction, OnClick, OnDrag, OnDragEnd, OnDragStart, OnKey, OnPaste},
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
    default_font: FontId,
    rect: Rect,
    input: InputState,
    cycle_focus_key: Option<Key>,
    start: Instant,
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
            default_bg_color: Color::TRANSPARENT,
            default_font: FontId(0),
            rect: Rect::ZERO,
            input: InputState::default(),
            cycle_focus_key: Some(Key::Tab),
            start: Instant::now(),
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

    pub fn default_font(&self) -> FontId {
        self.default_font
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
            font: ui.default_font,
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
    font: FontId,
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
    if inherited.stroke.is_some() {
        std::process::exit(0);
    }

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
        font: attrs.font.unwrap_or(inherited.font),
    };

    commands.extend(std::iter::once(Draw::Rect {
        geometry: rect,
        fill: Some(resolved.bg_brush),
        stroke: resolved.stroke,
    }));

    if let Ok((Some(text), text_input)) = world.get::<(Option<&Text>, Option<&TextInput>)>(id)
        && let Some(font) = ui.font(resolved.font)
    {
        let inner_margin = attrs.inner_margin.unwrap_or(ui.default_inner_margin());

        let content_align = attrs.content_align.unwrap_or(Align::Start.into());
        let text_rect = font.text_bbox(&text.string);

        // Inset the text by `inner_margin`, shrinking smoothly per axis (never a discrete
        // jump to zero margin) as the text grows to fill the widget's own rect — reuses the
        // exact same shrink used for cross-axis insets during layout, just applied here at
        // draw time against the widget's final rect and its own text content.
        let rect_size = rect.size();
        let text_size = text_rect.size();
        let (left_inset, right_inset) = shrink_cross_inset(
            inner_margin.left as i32,
            inner_margin.right as i32,
            rect_size.w,
            text_size.w,
        );
        let (top_inset, bottom_inset) = shrink_cross_inset(
            inner_margin.top as i32,
            inner_margin.bottom as i32,
            rect_size.h,
            text_size.h,
        );
        let content_rect = Rect {
            lt: Pos {
                x: rect.lt.x + left_inset,
                y: rect.lt.y + top_inset,
            },
            rb: Pos {
                x: rect.rb.x - right_inset,
                y: rect.rb.y - bottom_inset,
            },
        };

        let text_offset = content_align.rect_offset(content_rect, text_rect);

        match text_input {
            None => {
                let glyphs: Vec<Glyph> = text
                    .string
                    .chars()
                    .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                    .collect();

                commands.extend(std::iter::once(Draw::Text {
                    start: text_offset,
                    font: resolved.font,
                    glyphs: Cow::Owned(glyphs),
                    brush: resolved.fg_brush,
                }));
            }
            Some(text_input) => {
                let before_selection = &text.string[..text_input.selection.start];
                let selection = &text.string[text_input.selection.clone()];
                let after_selection = &text.string[text_input.selection.end..];

                let mut text_offset = text_offset;

                if !before_selection.is_empty() {
                    let glyphs: Vec<Glyph> = before_selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    let mut advance = 0;
                    for &g in glyphs.iter() {
                        advance += font.metrics(g).map_or(0, |m| m.advance.w as i32);
                    }

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));

                    text_offset.x += advance;
                }

                if text_input.selection.is_empty()
                    && ui.focused() == Some(id)
                    && (ui.start.elapsed().as_millis() % 1000 < 500)
                {
                    let x_offset = text_offset.x - 1;
                    let y_offset = content_rect.lt.y
                        + content_align
                            .y
                            .offset(content_rect.size().h, font.size.h + 2);

                    commands.extend(std::iter::once(Draw::Rect {
                        geometry: Rect::from_pos_size(
                            Pos {
                                x: x_offset,
                                y: y_offset,
                            },
                            Size {
                                w: 1,
                                h: font.size.h + 2,
                            },
                        ),

                        fill: Some(Brush::Solid(Color::from_premultiplied(100, 100, 100, 0))),
                        stroke: None,
                    }));
                }

                if !selection.is_empty() {
                    let glyphs: Vec<Glyph> = selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    let mut advance = 0;
                    for &g in glyphs.iter() {
                        advance += font.metrics(g).map_or(0, |m| m.advance.w as i32);
                    }

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));

                    text_offset.x += advance;
                }

                if !after_selection.is_empty() {
                    let glyphs: Vec<Glyph> = after_selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));
                }
            }
        }
    }

    let children = world
        .get::<Option<&Container>>(id)
        .ok()
        .flatten()
        .map(|c| c.children.clone());

    let child_paint = InheritedPaint {
        fg_brush: resolved.fg_brush,
        bg_brush: inherited.bg_brush,
        stroke: inherited.stroke,
        font: resolved.font,
    };

    if let Some(children) = children {
        for child in children {
            draw_widget(world, ui, child, child_paint, commands);
        }
    }
}

pub fn handle_event(world: &mut World, event: PixieEvent) {
    for a in handle_event_with_actions::<NoAction>(world, event) {
        match a {}
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
/// Releasing a click on a [`FocusOnClick`]-marked widget focuses it. Any other key press is
/// forwarded to the focused widget's [`OnKey<A>`], and [`PixieEvent::Paste`] is forwarded to
/// the focused widget's [`OnPaste<A>`] — neither is hit-tested, both go straight to
/// [`InputState::focused`].
///
/// Returns every action emitted in response to this event — currently a completed click (press
/// and release both landing on the same widget), read off that widget's
/// [`crate::action::OnClick<A>`] component, if present; a key press forwarded to the focused
/// widget's [`OnKey<A>`]; or a paste forwarded to the focused widget's [`OnPaste<A>`].
pub fn handle_event_with_actions<A: 'static>(
    world: &mut World,
    event: PixieEvent,
) -> impl Iterator<Item = A> {
    let mut actions = smallvec::SmallVec::<[A; 1]>::new();

    let Some(ui) = world.get_resource_mut::<Ui>() else {
        return actions.into_iter();
    };

    let mut input = ui.input;
    let cycle_focus_key = ui.cycle_focus_key;
    drop(ui);

    match event {
        PixieEvent::CursorMoved { pos } => {
            let old_pos = input.cursor;
            input.cursor = Some(pos);
            input.hovered = hit_test::<SensesCursor>(world, pos);

            if let Some(id) = input.pressed
                && let Some(old_pos) = old_pos
            {
                let delta = pos - old_pos;
                let local = world.local();

                if let Ok(mut on_drag) = local.try_view_one::<&mut OnDrag<NoAction>>(id) {
                    if let Some(on_drag) = on_drag.get_mut() {
                        let None = on_drag.invoke(local, id, pos, delta);
                    }
                }

                if type_is_not_no_action::<A>() {
                    if let Ok(mut on_drag) = local.try_view_one::<&mut OnDrag<A>>(id) {
                        if let Some(on_drag) = on_drag.get_mut() {
                            actions.extend(on_drag.invoke(local, id, pos, delta));
                        }
                    }
                }
            }
        }
        PixieEvent::ButtonPressed => {
            if let Some(pos) = input.cursor {
                input.pressed = hit_test::<SensesClicks>(world, pos);

                if let Some(id) = input.pressed {
                    let local = world.local();

                    if let Ok(mut on_drag_start) =
                        local.try_view_one::<&mut OnDragStart<NoAction>>(id)
                    {
                        if let Some(on_drag_start) = on_drag_start.get_mut() {
                            let None = on_drag_start.invoke(local, id, pos);
                        }
                    }

                    if type_is_not_no_action::<A>() {
                        if let Ok(mut on_drag_start) =
                            local.try_view_one::<&mut OnDragStart<A>>(id)
                        {
                            if let Some(on_drag_start) = on_drag_start.get_mut() {
                                actions.extend(on_drag_start.invoke(local, id, pos));
                            }
                        }
                    }
                }
            }
        }
        PixieEvent::ButtonReleased => {
            if let Some(id) = input.pressed {
                if input.hovered == Some(id) {
                    if world.get::<&FocusOnClick>(id).is_ok() {
                        input.focused = Some(id);
                    }

                    let local = world.local();

                    if let Ok(mut on_click) = local.try_view_one::<&mut OnClick<NoAction>>(id) {
                        if let Some(on_click) = on_click.get_mut() {
                            let None = on_click.invoke(local, id);
                        }
                    }

                    if type_is_not_no_action::<A>() {
                        if let Ok(mut on_click) = local.try_view_one::<&mut OnClick<A>>(id) {
                            if let Some(on_click) = on_click.get_mut() {
                                actions.extend(on_click.invoke(local, id));
                            }
                        }
                    }
                }

                if let Some(pos) = input.cursor {
                    let local = world.local();

                    if let Ok(mut on_drag_end) = local.try_view_one::<&mut OnDragEnd<NoAction>>(id)
                    {
                        if let Some(on_drag_end) = on_drag_end.get_mut() {
                            let None = on_drag_end.invoke(local, id, pos);
                        }
                    }

                    if type_is_not_no_action::<A>() {
                        if let Ok(mut on_drag_end) = local.try_view_one::<&mut OnDragEnd<A>>(id) {
                            if let Some(on_drag_end) = on_drag_end.get_mut() {
                                actions.extend(on_drag_end.invoke(local, id, pos));
                            }
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
            } else if let Some(id) = input.focused {
                if let Ok(mut on_key) = world.try_view_one::<&mut OnKey<NoAction>>(id) {
                    if let Some(on_key) = on_key.get_mut() {
                        let None = on_key.invoke(world, id, key);
                    }
                }

                if type_is_not_no_action::<A>() {
                    if let Ok(mut on_key) = world.try_view_one::<&mut OnKey<A>>(id) {
                        if let Some(on_key) = on_key.get_mut() {
                            actions.extend(on_key.invoke(world, id, key));
                        }
                    }
                }
            }
        }
        PixieEvent::Paste(text) => {
            if let Some(id) = input.focused {
                if let Ok(mut on_paste) = world.try_view_one::<&mut OnPaste<NoAction>>(id) {
                    if let Some(on_paste) = on_paste.get_mut() {
                        let None = on_paste.invoke(world, id, &text);
                    }
                }

                if type_is_not_no_action::<A>() {
                    if let Ok(mut on_paste) = world.try_view_one::<&mut OnPaste<A>>(id) {
                        if let Some(on_paste) = on_paste.get_mut() {
                            actions.extend(on_paste.invoke(world, id, &text));
                        }
                    }
                }
            }
        }
    }

    if let Some(mut ui) = world.get_resource_mut::<Ui>() {
        ui.input = input;
    }

    actions.into_iter()
}

fn type_is_not_no_action<A: 'static>() -> bool {
    core::any::TypeId::of::<A>() != core::any::TypeId::of::<NoAction>()
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

    use super::*;
    use crate::{
        event::Key,
        focus::FocusCycle,
        math::Size,
        text::{TextInput, edit_on_key, edit_on_paste},
        trigger::{OnKey, OnPaste},
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

    fn spawn_text_input(world: &mut World) -> EntityId {
        world
            .spawn((
                Widget { parent: None },
                TextInput::new(),
                Text::new(String::new()),
                edit_on_key(),
                edit_on_paste(),
            ))
            .id()
    }

    #[test]
    fn focused_widget_receives_key_pressed_and_updates_input_text() {
        let mut world = World::new();
        let mut ui = Ui::new();
        let id = spawn_text_input(&mut world);
        ui.set_focus(id);
        world.insert_resource(ui);

        handle_event(&mut world, PixieEvent::KeyPressed(Key::Char('a')));

        assert_eq!(world.get::<&Text>(id).unwrap().string, "a");
    }

    #[test]
    fn unfocused_widget_does_not_receive_key_pressed() {
        let mut world = World::new();
        world.insert_resource(Ui::new());
        let id = spawn_text_input(&mut world);

        handle_event(&mut world, PixieEvent::KeyPressed(Key::Char('a')));

        assert_eq!(world.get::<&Text>(id).unwrap().string, "");
    }

    #[test]
    fn cycle_focus_key_press_is_not_forwarded_to_on_key() {
        let mut world = World::new();
        let mut ui = Ui::new();
        let id = spawn_text_input(&mut world);
        world.insert(id, FocusCycle).unwrap();
        ui.set_focus(id);
        world.insert_resource(ui);

        handle_event(&mut world, PixieEvent::KeyPressed(Key::Tab));

        assert_eq!(world.get::<&Text>(id).unwrap().string, "");
    }

    #[test]
    fn paste_on_focused_widget_updates_input_text() {
        let mut world = World::new();
        let mut ui = Ui::new();
        let id = spawn_text_input(&mut world);
        ui.set_focus(id);
        world.insert_resource(ui);

        handle_event(&mut world, PixieEvent::Paste("hello".into()));

        assert_eq!(world.get::<&Text>(id).unwrap().string, "hello");
    }

    #[test]
    fn tab_cycle_wraps_from_last_to_first() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let first = spawn_focus_cycle_widget(&mut world);
        let last = spawn_focus_cycle_widget(&mut world);

        world.get_resource_mut::<Ui>().unwrap().set_focus(last);
        handle_event(&mut world, PixieEvent::KeyPressed(Key::Tab));

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), Some(first));
    }

    #[test]
    fn tab_cycle_with_nothing_focused_picks_first() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let first = spawn_focus_cycle_widget(&mut world);
        let _second = spawn_focus_cycle_widget(&mut world);

        handle_event(&mut world, PixieEvent::KeyPressed(Key::Tab));

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

        handle_event(&mut world, PixieEvent::KeyPressed(Key::Tab));

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), None);
    }

    #[test]
    fn button_released_focuses_focus_on_click_widget() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let rect = Rect::from_pos_size(Pos::ZERO, Size { w: 10, h: 10 });
        let id = spawn_clickable(&mut world, true, rect);

        let pos = Pos { x: 5, y: 5 };
        handle_event(&mut world, PixieEvent::CursorMoved { pos });
        handle_event(&mut world, PixieEvent::ButtonPressed);
        handle_event(&mut world, PixieEvent::ButtonReleased);

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), Some(id));
    }

    #[test]
    fn button_released_ignores_widget_without_focus_on_click_marker() {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let rect = Rect::from_pos_size(Pos::ZERO, Size { w: 10, h: 10 });
        let _id = spawn_clickable(&mut world, false, rect);

        let pos = Pos { x: 5, y: 5 };
        handle_event(&mut world, PixieEvent::CursorMoved { pos });
        handle_event(&mut world, PixieEvent::ButtonPressed);
        handle_event(&mut world, PixieEvent::ButtonReleased);

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
        handle_event(
            &mut world,
            PixieEvent::CursorMoved {
                pos: Pos { x: 5, y: 5 },
            },
        );
        handle_event(&mut world, PixieEvent::ButtonPressed);
        handle_event(
            &mut world,
            PixieEvent::CursorMoved {
                pos: Pos { x: 25, y: 25 },
            },
        );
        handle_event(&mut world, PixieEvent::ButtonReleased);

        assert_eq!(world.get_resource::<Ui>().unwrap().focused(), None);
    }
}
