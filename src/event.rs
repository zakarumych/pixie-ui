use edict::{component::Component, entity::EntityId, query::Entities, world::World};

use crate::{
    focus::{FocusOnClick, collect_focus_cycle_order},
    layout::Arranged,
    math::Pos,
    trigger::{NoAction, OnClick, OnDrag, OnDragEnd, OnDragStart, OnKey, OnPaste},
    ui::Ui,
    widget::{SensesClicks, SensesCursor},
};

/// Keys pixie-ui reacts to. Deliberately minimal for now — more can be added later
/// without breaking this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Tab,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    Char(char),
}

/// Input events pixie-ui reacts to. Deliberately minimal for now — just enough
/// to drive hover/press/focus state. More event kinds (scroll, etc.) can be
/// added later without breaking this.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PixieEvent {
    CursorMoved { pos: Pos },
    ButtonPressed,
    ButtonReleased,
    KeyPressed(Key),
    Paste(String),
}

#[inline]
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
    let world = world.local();
    let mut actions = smallvec::SmallVec::<[A; 1]>::new();

    let Some(ui) = world.get_resource_mut::<Ui>() else {
        return actions.into_iter();
    };

    let mut input = ui.input();
    let cycle_focus_key = ui.cycle_focus_key();
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

                if !input.is_dragging {
                    input.is_dragging = true;

                    if let Ok(mut on_drag_start) =
                        world.try_view_one::<&mut OnDragStart<NoAction>>(id)
                    {
                        if let Some(on_drag_start) = on_drag_start.get_mut() {
                            let None = on_drag_start.trigger(world, id, pos, delta);
                        }
                    }

                    if type_is_not_no_action::<A>() {
                        if let Ok(mut on_drag_start) = world.try_view_one::<&mut OnDragStart<A>>(id)
                        {
                            if let Some(on_drag_start) = on_drag_start.get_mut() {
                                actions.extend(on_drag_start.trigger(world, id, pos, delta));
                            }
                        }
                    }
                }

                if let Ok(mut on_drag) = world.try_view_one::<&mut OnDrag<NoAction>>(id) {
                    if let Some(on_drag) = on_drag.get_mut() {
                        let None = on_drag.trigger(world, id, pos, delta);
                    }
                }

                if type_is_not_no_action::<A>() {
                    if let Ok(mut on_drag) = world.try_view_one::<&mut OnDrag<A>>(id) {
                        if let Some(on_drag) = on_drag.get_mut() {
                            actions.extend(on_drag.trigger(world, id, pos, delta));
                        }
                    }
                }
            }
        }
        PixieEvent::ButtonPressed => {
            if let Some(pos) = input.cursor {
                input.pressed = hit_test::<SensesClicks>(world, pos);
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
                            let None = on_click.trigger(local, id);
                        }
                    }

                    if type_is_not_no_action::<A>() {
                        if let Ok(mut on_click) = local.try_view_one::<&mut OnClick<A>>(id) {
                            if let Some(on_click) = on_click.get_mut() {
                                actions.extend(on_click.trigger(local, id));
                            }
                        }
                    }
                }

                if input.is_dragging {
                    input.is_dragging = false;

                    if let Some(pos) = input.cursor {
                        let local = world.local();

                        if let Ok(mut on_drag_end) =
                            local.try_view_one::<&mut OnDragEnd<NoAction>>(id)
                        {
                            if let Some(on_drag_end) = on_drag_end.get_mut() {
                                let None = on_drag_end.trigger(local, id, pos);
                            }
                        }

                        if type_is_not_no_action::<A>() {
                            if let Ok(mut on_drag_end) = local.try_view_one::<&mut OnDragEnd<A>>(id)
                            {
                                if let Some(on_drag_end) = on_drag_end.get_mut() {
                                    actions.extend(on_drag_end.trigger(local, id, pos));
                                }
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
                        let None = on_key.trigger(world, id, key);
                    }
                }

                if type_is_not_no_action::<A>() {
                    if let Ok(mut on_key) = world.try_view_one::<&mut OnKey<A>>(id) {
                        if let Some(on_key) = on_key.get_mut() {
                            actions.extend(on_key.trigger(world, id, key));
                        }
                    }
                }
            }
        }
        PixieEvent::Paste(text) => {
            if let Some(id) = input.focused {
                if type_is_not_no_action::<A>() {
                    if let Ok(mut view) =
                        world.try_view_one::<(&mut OnPaste<A>, &mut OnPaste<NoAction>)>(id)
                    {
                        if let Some((on_paste, on_paste_no_action)) = view.get_mut() {
                            let None = on_paste_no_action.trigger(world, id, text.clone());
                            actions.extend(on_paste.trigger(world, id, text));
                        }
                    }
                } else {
                    if let Ok(mut on_paste) = world.try_view_one::<&mut OnPaste<NoAction>>(id) {
                        if let Some(on_paste) = on_paste.get_mut() {
                            let None = on_paste.trigger(world, id, text);
                        }
                    }
                }
            }
        }
    }

    if let Some(mut ui) = world.get_resource_mut::<Ui>() {
        ui.set_input(input);
    }

    actions.into_iter()
}

fn type_is_not_no_action<A: 'static>() -> bool {
    core::any::TypeId::of::<A>() != core::any::TypeId::of::<NoAction>()
}

/// Returns the entity with the deepest `Arranged.layer` (i.e. visually top-most)
/// among all `M`-marked widgets whose `Arranged.rect` contains `pos`, or `None`.
fn hit_test<M: Component>(world: &World, pos: Pos) -> Option<EntityId> {
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
}
