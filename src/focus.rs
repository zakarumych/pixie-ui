//! Focus-related marker components for use with [`crate::ui::Ui`]'s focus handling.
//!
//! Focus handling itself (Tab-cycling, click-to-focus) lives in [`crate::ui::handle_event`],
//! which is the single entry point for feeding [`crate::event::PixieEvent`]s into the UI. This
//! module only holds the marker components consumers spawn widgets with ([`FocusCycle`],
//! [`FocusOnClick`]) plus a `pub(crate)` tree-order helper used internally by
//! `ui::handle_event`.

use edict::{component::Component, entity::EntityId, query::Entities, world::World};

use crate::widget::{Container, Widget};

/// Marker: this widget participates in Tab-cycling (or whatever key is configured via
/// [`crate::ui::Ui::set_cycle_focus_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct FocusCycle;

/// Marker: this widget is focused when clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct FocusOnClick;

/// Collects every [`FocusCycle`]-marked entity, root-to-leaf, in [`Container::children`]
/// order, across all root widgets (a root = a [`Widget`] whose `parent` is `None`).
pub(crate) fn collect_focus_cycle_order(world: &mut World) -> Vec<EntityId> {
    let roots: Vec<EntityId> = world
        .view::<(Entities, &Widget)>()
        .into_iter()
        .filter(|(_, w)| w.parent.is_none())
        .map(|(e, _)| e.id())
        .collect();

    let mut order = Vec::new();
    for root in roots {
        collect_widget(world, root, &mut order);
    }
    order
}

fn collect_widget(world: &mut World, id: EntityId, order: &mut Vec<EntityId>) {
    if world.get::<&FocusCycle>(id).is_ok() {
        order.push(id);
    }

    let children = world
        .get::<Option<&Container>>(id)
        .ok()
        .flatten()
        .map(|c| c.children.clone());

    if let Some(children) = children {
        for child in children {
            collect_widget(world, child, order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_container(world: &mut World, children: Vec<EntityId>) -> EntityId {
        world
            .spawn((Widget { parent: None }, Container { children }))
            .id()
    }

    fn spawn_focus_cycle_widget(world: &mut World, parent: Option<EntityId>) -> EntityId {
        world.spawn((Widget { parent }, FocusCycle)).id()
    }

    fn spawn_plain_widget(world: &mut World, parent: Option<EntityId>) -> EntityId {
        world.spawn((Widget { parent },)).id()
    }

    #[test]
    fn collect_focus_cycle_order_on_empty_world_is_empty() {
        let mut world = World::new();

        assert_eq!(collect_focus_cycle_order(&mut world), Vec::<EntityId>::new());
    }

    #[test]
    fn collect_focus_cycle_order_with_no_focus_cycle_widgets_is_empty() {
        let mut world = World::new();
        let _plain = spawn_plain_widget(&mut world, None);

        assert_eq!(collect_focus_cycle_order(&mut world), Vec::<EntityId>::new());
    }

    #[test]
    fn collect_focus_cycle_order_returns_depth_first_order_skipping_non_focus_cycle_widgets() {
        let mut world = World::new();

        // root
        //   - a (FocusCycle)
        //     - a1 (plain, skipped)
        //     - a2 (FocusCycle)
        //   - b (plain, skipped)
        //   - c (FocusCycle)
        let a1 = spawn_plain_widget(&mut world, None);
        let a2 = spawn_focus_cycle_widget(&mut world, None);
        let a = world
            .spawn((
                Widget { parent: None },
                Container {
                    children: vec![a1, a2],
                },
                FocusCycle,
            ))
            .id();
        let b = spawn_plain_widget(&mut world, None);
        let c = spawn_focus_cycle_widget(&mut world, None);
        let _root = spawn_container(&mut world, vec![a, b, c]);
        crate::widget::sync_widget_parents(&mut world);

        assert_eq!(collect_focus_cycle_order(&mut world), vec![a, a2, c]);
    }
}
