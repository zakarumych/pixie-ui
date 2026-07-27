use std::convert::Infallible;

use edict::{component::Component, entity::EntityId, world::World};

tiny_fn::tiny_fn! {
    pub struct ActionFn<A> = FnMut(world: &World, id: EntityId) -> Option<A>| + Send;
}

const TRIGGER_INLINE_SIZE: usize = std::mem::size_of::<usize>();

pub type Action<A> = ActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to emit `A` when it's clicked — press and release both
/// landing on the same widget (see [`crate::ui::handle_event`]).
pub struct OnClick<A>(pub Action<A>);

impl<A> Component for OnClick<A> where A: 'static {}

impl<A> OnClick<A> {
    pub(crate) fn invoke(&mut self, world: &World, id: EntityId) -> Option<A> {
        (self.0).call(world, id)
    }
}

pub fn emit<A>(action: A) -> Action<A>
where
    A: Copy + Send + 'static,
{
    ActionFn::new(move |_world, _id| Some(action))
}

pub fn invoke<F>(mut fun: F) -> Action<Infallible>
where
    F: FnMut(&World, EntityId) + Send + 'static,
{
    ActionFn::new(move |world, id| {
        fun(world, id);
        None
    })
}
