use std::{num::NonZeroU64, time::Instant};

use edict::entity::EntityId;
use foldhash::fast::RandomState;
use hashbrown::HashMap;

use crate::{
    align::{Align, Align2},
    color::Color,
    event::Key,
    font::{Font, FontId},
    layout::ContentLayout,
    margin::Margin,
    math::{Pos, Rect},
    texture::TextureId,
};

#[derive(Clone, Copy, Default)]
pub struct InputState {
    pub focused: Option<EntityId>,
    pub hovered: Option<EntityId>,
    pub pressed: Option<EntityId>,
    pub cursor: Option<Pos>,
    pub is_dragging: bool,
}

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

    pub fn input(&self) -> InputState {
        self.input
    }

    pub fn set_input(&mut self, input: InputState) {
        self.input = input;
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

    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {

    use edict::world::World;

    use super::*;
    use crate::{
        event::{Key, PixieEvent, handle_event},
        focus::{FocusCycle, FocusOnClick},
        layout::Arranged,
        math::{Pos, Size},
        text::{Text, TextInput, edit_on_key, edit_on_paste},
        widget::{SensesClicks, SensesCursor, Widget},
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
