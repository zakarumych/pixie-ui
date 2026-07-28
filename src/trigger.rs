use edict::{
    component::Component,
    entity::EntityId,
    world::{World, WorldLocal},
};

/// Uninhabited action type.
pub enum NoAction {}

use crate::event::Key;
use crate::math::{Pos, Vec};

tiny_fn::tiny_fn! {
    pub struct ActionFn<A> = FnMut(world: &WorldLocal, id: EntityId) -> Option<A>| + Send;
}

const TRIGGER_INLINE_SIZE: usize = std::mem::size_of::<usize>();

pub type Action<A> = ActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to emit `A` when it's clicked — press and release both
/// landing on the same widget (see [`crate::ui::handle_event`]).
pub struct OnClick<A = NoAction>(pub Action<A>);

impl<A> Component for OnClick<A> where A: 'static {}

impl<A> OnClick<A> {
    pub(crate) fn invoke(&mut self, world: &WorldLocal, id: EntityId) -> Option<A> {
        (self.0).call(world, id)
    }
}

pub fn emit<A>(action: A) -> Action<A>
where
    A: Copy + Send + 'static,
{
    ActionFn::new(move |_world, _id| Some(action))
}

pub fn invoke<F>(mut fun: F) -> Action<NoAction>
where
    F: FnMut(&WorldLocal, EntityId) + Send + 'static,
{
    ActionFn::new(move |world, id| {
        fun(world, id);
        None
    })
}

tiny_fn::tiny_fn! {
    pub struct KeyActionFn<A> = FnMut(world: &World, id: EntityId, key: Key) -> Option<A> | + Send;
}

pub type KeyAction<A> = KeyActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to react to a key press while it holds focus (see [`crate::ui::handle_event`]).
pub struct OnKey<A = NoAction>(pub KeyAction<A>);

impl<A> Component for OnKey<A> where A: 'static {}

impl<A> OnKey<A> {
    pub(crate) fn invoke(&mut self, world: &World, id: EntityId, key: Key) -> Option<A> {
        (self.0).call(world, id, key)
    }
}

tiny_fn::tiny_fn! {
    pub struct PasteActionFn<A> = FnMut(world: &World, id: EntityId, text: &str) -> Option<A> | + Send;
}

pub type PasteAction<A> = PasteActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to react to a paste while it holds focus (see [`crate::ui::handle_event`]).
pub struct OnPaste<A = NoAction>(pub PasteAction<A>);

impl<A> Component for OnPaste<A> where A: 'static {}

impl<A> OnPaste<A> {
    pub(crate) fn invoke(&mut self, world: &World, id: EntityId, text: &str) -> Option<A> {
        (self.0).call(world, id, text)
    }
}

pub fn invoke_key<F>(mut fun: F) -> KeyAction<NoAction>
where
    F: FnMut(&World, EntityId, Key) + Send + 'static,
{
    KeyActionFn::new(move |world, id, key| {
        fun(world, id, key);
        None
    })
}

pub fn invoke_paste<F>(mut fun: F) -> PasteAction<NoAction>
where
    F: FnMut(&World, EntityId, &str) + Send + 'static,
{
    PasteActionFn::new(move |world, id, text| {
        fun(world, id, text);
        None
    })
}

tiny_fn::tiny_fn! {
    pub struct DragActionFn<A> = FnMut(world: &WorldLocal, id: EntityId, pos: Pos, delta: Vec) -> Option<A> | + Send;
}

pub type DragAction<A> = DragActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to react to cursor movement while the mouse button is held and this
/// widget is the pressed target (see [`crate::ui::handle_event`]). Fires continuously — once per
/// `CursorMoved` — for as long as the button stays down, regardless of whether the cursor stays
/// within the widget's own bounds; the drag ends only when the button is released.
///
/// Not mutually exclusive with [`OnClick`] on the same widget: a press → drag → release-back-
/// over-the-widget sequence fires both, with no built-in distance threshold between them.
pub struct OnDrag<A = NoAction>(pub DragAction<A>);

impl<A> Component for OnDrag<A> where A: 'static {}

impl<A> OnDrag<A> {
    pub(crate) fn invoke(
        &mut self,
        world: &WorldLocal,
        id: EntityId,
        pos: Pos,
        delta: Vec,
    ) -> Option<A> {
        (self.0).call(world, id, pos, delta)
    }
}

pub fn invoke_drag<F>(mut fun: F) -> DragAction<NoAction>
where
    F: FnMut(&WorldLocal, EntityId, Pos, Vec) + Send + 'static,
{
    DragActionFn::new(move |world, id, pos, delta| {
        fun(world, id, pos, delta);
        None
    })
}

tiny_fn::tiny_fn! {
    pub struct DragStartActionFn<A> = FnMut(world: &WorldLocal, id: EntityId, pos: Pos) -> Option<A> | + Send;
}

pub type DragStartAction<A> = DragStartActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to react to the moment it becomes the pressed target (see
/// [`crate::ui::handle_event`]) — fires once per press, before any [`OnDrag`] events for that
/// same gesture, regardless of whether the cursor ever actually moves. Pairs with [`OnDragEnd`],
/// which fires once when the button is released, whether or not this fired first.
///
/// Not mutually exclusive with [`OnClick`]: both are driven by the same press/release, just
/// unconditionally (`OnDragStart`/`OnDragEnd`) vs. only when release lands back on the widget
/// (`OnClick`).
pub struct OnDragStart<A = NoAction>(pub DragStartAction<A>);

impl<A> Component for OnDragStart<A> where A: 'static {}

impl<A> OnDragStart<A> {
    pub(crate) fn invoke(&mut self, world: &WorldLocal, id: EntityId, pos: Pos) -> Option<A> {
        (self.0).call(world, id, pos)
    }
}

pub fn invoke_drag_start<F>(mut fun: F) -> DragStartAction<NoAction>
where
    F: FnMut(&WorldLocal, EntityId, Pos) + Send + 'static,
{
    DragStartActionFn::new(move |world, id, pos| {
        fun(world, id, pos);
        None
    })
}

tiny_fn::tiny_fn! {
    pub struct DragEndActionFn<A> = FnMut(world: &WorldLocal, id: EntityId, pos: Pos) -> Option<A> | + Send;
}

pub type DragEndAction<A> = DragEndActionFn<'static, A, TRIGGER_INLINE_SIZE>;

/// Attach to a widget to react to the moment it stops being the pressed target (see
/// [`crate::ui::handle_event`]) — fires once per release, regardless of where the cursor ends up
/// or whether any [`OnDrag`] events fired in between. See [`OnDragStart`] for the paired
/// press-time trigger.
pub struct OnDragEnd<A = NoAction>(pub DragEndAction<A>);

impl<A> Component for OnDragEnd<A> where A: 'static {}

impl<A> OnDragEnd<A> {
    pub(crate) fn invoke(&mut self, world: &WorldLocal, id: EntityId, pos: Pos) -> Option<A> {
        (self.0).call(world, id, pos)
    }
}

pub fn invoke_drag_end<F>(mut fun: F) -> DragEndAction<NoAction>
where
    F: FnMut(&WorldLocal, EntityId, Pos) + Send + 'static,
{
    DragEndActionFn::new(move |world, id, pos| {
        fun(world, id, pos);
        None
    })
}
