use edict::{
    action::LocalActionEncoder, component::Component, entity::EntityId, epoch::EpochId,
    query::Entities, world::World,
};
use smallvec::SmallVec;

/// Main component for a widget.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Widget {
    pub parent: Option<EntityId>,
}

impl Component for Widget {
    fn name() -> &'static str {
        "Widget"
    }

    fn on_replace(&mut self, value: &Self, _id: EntityId, _encoder: LocalActionEncoder) -> bool
    where
        Self: Sized,
    {
        self.parent != value.parent
    }

    fn on_drop(&mut self, id: EntityId, mut encoder: LocalActionEncoder) {
        if let Some(parent) = self.parent {
            encoder.closure(move |w| {
                if let Ok(container) = w.get::<&mut Container>(parent) {
                    container.children.retain(|&c| c != id);
                }
            });
        }
    }
}

/// Widget component for widgets that can contain other widgets.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Container {
    /// The child widgets of this container.
    pub children: Vec<EntityId>,
}

impl Component for Container {
    fn name() -> &'static str {
        "Container"
    }

    fn on_drop(&mut self, _: EntityId, mut encoder: LocalActionEncoder) {
        encoder.despawn_batch(std::mem::take(&mut self.children));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct RootWidget;

/// Marker: this widget participates in cursor hover hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct SensesCursor;

/// Marker: this widget participates in click hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct SensesClicks;

/// Epoch cursors for [`sync_widget_parents`], so it only re-examines entities touched since
/// its own last run rather than the whole world.
#[derive(Debug, Default, Clone, Copy)]
struct SyncEpochs {
    containers: EpochId,
    widgets: EpochId,
}

/// Keeps `Widget::parent` consistent with `Container::children`, which is the source of truth.
///
/// Each pass indexes into a second view (of a different component type) fetched in parallel
/// through the shared `&World`, rather than collecting the whole working set into a temporary
/// buffer:
///
/// 1. For every `Container` modified since the last run, point each listed child's
///    `Widget::parent` at that container. A listed child that has no `Widget` component at all
///    was never a valid child to begin with, so it's simply dropped from `children`.
/// 2. `children` is a list of entities the container *owns* (mirroring
///    [`Container::on_drop`], which despawns all of them when the container itself is
///    dropped), so a `Widget` that has a `parent` but is no longer listed in that parent's
///    `children` — because it was removed there directly, not via this widget — has lost its
///    owner and is despawned, not merely detached.
///
///    Removal like that doesn't touch the `Widget` component itself, only the `Container`, so
///    it can't be scoped with [`Modified`](edict::query::Modified) on `Widget` alone: pass 2 is
///    scoped to `Modified<Widget>` (catches a widget's own `parent` being changed directly)
///    plus, only on runs where pass 1 actually touched some container, a full scan of widgets
///    that have a `parent` at all (catches a widget silently dropped from its container).
pub fn sync_widget_parents(world: &mut World) {
    let epochs = *world.with_default_resource::<SyncEpochs>();

    let mut any_container_touched = false;
    {
        let mut containers = world
            .view::<Entities>()
            .modified_mut::<Container>(epochs.containers);
        let mut widgets = world.view::<&mut Widget>();

        for (container_id, container) in containers.iter_mut() {
            any_container_touched = true;
            let container_id = container_id.id();
            container
                .children
                .retain(|&child| match widgets.try_get_mut(child) {
                    Ok(widget) => {
                        widget.parent = Some(container_id);
                        true
                    }
                    Err(_) => false,
                });
        }
    }

    let mut despawn: SmallVec<[EntityId; 4]> = SmallVec::new();
    {
        let mut containers = world.view::<&Container>();
        let containers = containers.lock();

        let is_orphaned = |widget_id: EntityId, parent: EntityId| {
            !containers
                .try_get(parent)
                .is_ok_and(|c| c.children.contains(&widget_id))
        };

        if any_container_touched {
            // Some container's `children` changed this run: a widget could have been quietly
            // dropped from it, which doesn't touch that `Widget` component, so every widget
            // with a `parent` needs rechecking.
            let mut widgets = world.view::<(Entities, &mut Widget)>();
            for (widget_id, widget) in widgets.iter_mut() {
                let widget_id = widget_id.id();
                if widget
                    .parent
                    .is_some_and(|parent| is_orphaned(widget_id, parent))
                {
                    despawn.push(widget_id);
                }
            }
        } else {
            let mut widgets = world
                .view::<Entities>()
                .modified_mut::<Widget>(epochs.widgets);
            for (widget_id, widget) in widgets.iter_mut() {
                let widget_id = widget_id.id();
                if widget
                    .parent
                    .is_some_and(|parent| is_orphaned(widget_id, parent))
                {
                    despawn.push(widget_id);
                }
            }
        }
    }
    for widget_id in despawn {
        let _ = world.despawn(widget_id);
    }

    // Fetched last, after every write this run: otherwise our own writes above would push the
    // world epoch past `now` before it's recorded, and next run would see entities we just
    // touched ourselves as "modified" for no reason.
    let now = world.epoch();
    let epochs = world.with_default_resource::<SyncEpochs>();
    epochs.containers = now;
    epochs.widgets = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_container(world: &mut World, children: Vec<EntityId>) -> EntityId {
        world
            .spawn((Widget { parent: None }, Container { children }))
            .id()
    }

    fn spawn_widget(world: &mut World, parent: Option<EntityId>) -> EntityId {
        world.spawn((Widget { parent },)).id()
    }

    #[test]
    fn container_children_set_widget_parent() {
        let mut world = World::new();
        let child = spawn_widget(&mut world, None);
        let container = spawn_container(&mut world, vec![child]);

        sync_widget_parents(&mut world);

        assert_eq!(world.get::<&Widget>(child).unwrap().parent, Some(container));
    }

    #[test]
    fn moving_child_between_containers_follows_the_authoritative_list() {
        let mut world = World::new();
        let child = spawn_widget(&mut world, None);
        let old = spawn_container(&mut world, vec![child]);
        sync_widget_parents(&mut world);
        assert_eq!(world.get::<&Widget>(child).unwrap().parent, Some(old));

        // Container::children is authoritative: the caller, not the sync system, is
        // responsible for removing `child` from its old container when relisting it
        // under a new one.
        world.get::<&mut Container>(old).unwrap().children.clear();
        let new = spawn_container(&mut world, vec![child]);
        sync_widget_parents(&mut world);

        assert_eq!(world.get::<&Widget>(child).unwrap().parent, Some(new));
    }

    #[test]
    fn child_missing_widget_component_is_pruned_but_not_despawned() {
        let mut world = World::new();
        let not_a_widget = world.spawn(()).id();
        let container = spawn_container(&mut world, vec![not_a_widget]);

        sync_widget_parents(&mut world);

        assert!(
            !world
                .get::<&Container>(container)
                .unwrap()
                .children
                .contains(&not_a_widget)
        );
        assert!(world.is_alive(not_a_widget));
    }

    #[test]
    fn widget_with_stale_parent_gets_despawned() {
        let mut world = World::new();
        let container = spawn_container(&mut world, vec![]);
        let widget = spawn_widget(&mut world, Some(container));

        sync_widget_parents(&mut world);

        assert!(!world.is_alive(widget));
    }

    #[test]
    fn widget_silently_dropped_from_container_children_gets_despawned() {
        let mut world = World::new();
        let child = spawn_widget(&mut world, None);
        let container = spawn_container(&mut world, vec![child]);
        sync_widget_parents(&mut world);
        assert_eq!(world.get::<&Widget>(child).unwrap().parent, Some(container));

        // Directly drop `child` from `container.children` without touching `Widget` at all.
        // `child` still (wrongly) believes it belongs to `container`. This mutation is itself
        // what makes pass 1 see `container` as modified next run, which is what makes pass 2
        // fall back to its full, un-`Modified`-scoped sweep.
        world
            .get::<&mut Container>(container)
            .unwrap()
            .children
            .clear();
        sync_widget_parents(&mut world);

        assert!(!world.is_alive(child));
    }

    #[test]
    fn settled_tree_is_stable_across_repeated_runs() {
        let mut world = World::new();
        let child = spawn_widget(&mut world, None);
        let container = spawn_container(&mut world, vec![child]);

        for _ in 0..3 {
            sync_widget_parents(&mut world);
            assert_eq!(world.get::<&Widget>(child).unwrap().parent, Some(container));
            assert!(
                world
                    .get::<&Container>(container)
                    .unwrap()
                    .children
                    .contains(&child)
            );
        }
    }

    #[test]
    fn modifying_one_container_does_not_disturb_another() {
        let mut world = World::new();
        let child_a = spawn_widget(&mut world, None);
        let container_a = spawn_container(&mut world, vec![child_a]);
        let child_b = spawn_widget(&mut world, None);
        let container_b = spawn_container(&mut world, vec![child_b]);
        sync_widget_parents(&mut world);

        let extra = spawn_widget(&mut world, None);
        world
            .get::<&mut Container>(container_b)
            .unwrap()
            .children
            .push(extra);
        sync_widget_parents(&mut world);

        assert_eq!(
            world.get::<&Widget>(extra).unwrap().parent,
            Some(container_b)
        );
        assert_eq!(
            world.get::<&Widget>(child_a).unwrap().parent,
            Some(container_a)
        );
    }
}
