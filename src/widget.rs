use edict::{action::LocalActionEncoder, component::Component, entity::EntityId};

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
