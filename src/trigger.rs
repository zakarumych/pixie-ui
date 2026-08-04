use edict::{component::Component, entity::EntityId, world::WorldLocal};

use crate::{
    event::Key,
    math::{Pos, Vec},
};

/// Uninhabited action type.
pub enum NoAction {}

tiny_fn::tiny_fn! {
    pub struct HandlerFn<R, A> = FnMut(world: &WorldLocal, id: EntityId, args: A) -> Option<R>| + Send;
}

const TRIGGER_INLINE_SIZE: usize = std::mem::size_of::<usize>();

pub type Handler<R, A = ()> = HandlerFn<'static, R, A, TRIGGER_INLINE_SIZE>;

#[inline(always)]
pub fn emit<R, A>(signal: R) -> Handler<R, A>
where
    R: Copy + Send + 'static,
{
    HandlerFn::new(move |_world, _id, _args| Some(signal))
}

// /// Helper trait for [`fn invoke`] that is implemented for functions of various arities.
// /// Used to allow the same `invoke` function to be used for all of them.
// pub trait Invoke<A>: Send + 'static {
//     fn invoke(&mut self, world: &WorldLocal, id: EntityId, args: A);
// }

// macro_rules! invoke {
//     ($($a:ident)*) => {
//         impl<Fun $(, $a)*> Invoke<($($a,)*)> for Fun
//         where
//             Fun: FnMut(&WorldLocal, EntityId $(, $a)*) + Send + 'static,
//         {
//             #[inline]
//             fn invoke(
//                 &mut self,
//                 world: &WorldLocal,
//                 id: EntityId,
//                 args: ($($a,)*),
//             ) {
//                 #[allow(non_snake_case)]

//                 let ($($a,)*) = args;
//                 (self)(world, id $(, $a)*);
//             }
//         }
//     };
// }

// for_tuple!(invoke);

// #[inline(always)]
// pub fn invoke<F, A>(mut fun: F) -> Handler<NoAction, A>
// where
//     F: Invoke<A>,
// {
//     HandlerFn::new(move |world, id, args| {
//         fun.invoke(world, id, args);
//         None
//     })
// }

/// Attach to a widget to emit `A` when it's clicked — press and release both
/// landing on the same widget (see [`crate::ui::handle_event`]).
pub struct OnClick<R = NoAction>(pub Handler<R>);

impl<R> Component for OnClick<R> where R: 'static {}

impl OnClick {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnClick
    where
        F: FnMut(&WorldLocal, EntityId) + Send + 'static,
    {
        OnClick(HandlerFn::new(move |world, id, ()| {
            (fun)(world, id);
            None
        }))
    }
}

impl<R> OnClick<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnClick<R>
    where
        R: Copy + Send + 'static,
    {
        OnClick(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(&mut self, world: &WorldLocal, id: EntityId) -> Option<R> {
        (self.0).call(world, id, ())
    }
}

/// Attach to a widget to react to a key press while it holds focus (see [`crate::ui::handle_event`]).
pub struct OnKey<R = NoAction>(pub Handler<R, (Key,)>);

impl<R> Component for OnKey<R> where R: 'static {}

impl OnKey {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnKey
    where
        F: FnMut(&WorldLocal, EntityId, Key) + Send + 'static,
    {
        OnKey(HandlerFn::new(move |world, id, (key,)| {
            (fun)(world, id, key);
            None
        }))
    }
}

impl<R> OnKey<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnKey<R>
    where
        R: Copy + Send + 'static,
    {
        OnKey(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(&mut self, world: &WorldLocal, id: EntityId, key: Key) -> Option<R> {
        (self.0).call(world, id, (key,))
    }
}

/// Attach to a widget to react to a paste while it holds focus (see [`crate::ui::handle_event`]).
pub struct OnPaste<R = NoAction>(pub Handler<R, (String,)>);

impl OnPaste {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnPaste
    where
        F: FnMut(&WorldLocal, EntityId, String) + Send + 'static,
    {
        OnPaste(HandlerFn::new(move |world, id, (text,)| {
            (fun)(world, id, text);
            None
        }))
    }
}

impl<R> Component for OnPaste<R> where R: 'static {}

impl<R> OnPaste<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnPaste<R>
    where
        R: Copy + Send + 'static,
    {
        OnPaste(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(&mut self, world: &WorldLocal, id: EntityId, text: String) -> Option<R> {
        (self.0).call(world, id, (text,))
    }
}

/// Attach to a widget to react to cursor movement while the mouse button is held and this
/// widget is the pressed target (see [`crate::ui::handle_event`]). Fires continuously — once per
/// `CursorMoved` — for as long as the button stays down, regardless of whether the cursor stays
/// within the widget's own bounds; the drag ends only when the button is released.
///
/// Not mutually exclusive with [`OnClick`] on the same widget: a press → drag → release-back-
/// over-the-widget sequence fires both, with no built-in distance threshold between them.
pub struct OnDrag<R = NoAction>(pub Handler<R, (Pos, Vec)>);

impl<R> Component for OnDrag<R> where R: 'static {}

impl OnDrag {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnDrag
    where
        F: FnMut(&WorldLocal, EntityId, Pos, Vec) + Send + 'static,
    {
        OnDrag(HandlerFn::new(move |world, id, (pos, delta)| {
            (fun)(world, id, pos, delta);
            None
        }))
    }
}

impl<R> OnDrag<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnDrag<R>
    where
        R: Copy + Send + 'static,
    {
        OnDrag(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(
        &mut self,
        world: &WorldLocal,
        id: EntityId,
        pos: Pos,
        delta: Vec,
    ) -> Option<R> {
        (self.0).call(world, id, (pos, delta))
    }
}

/// Attach to a widget to react to the moment it becomes the pressed target (see
/// [`crate::ui::handle_event`]) — fires once per press, before any [`OnDrag`] events for that
/// same gesture, regardless of whether the cursor ever actually moves. Pairs with [`OnDragEnd`],
/// which fires once when the button is released, whether or not this fired first.
///
/// Not mutually exclusive with [`OnClick`]: both are driven by the same press/release, just
/// unconditionally (`OnDragStart`/`OnDragEnd`) vs. only when release lands back on the widget
/// (`OnClick`).
pub struct OnDragStart<R = NoAction>(pub Handler<R, (Pos, Vec)>);

impl<R> Component for OnDragStart<R> where R: 'static {}

impl OnDragStart {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnDragStart
    where
        F: FnMut(&WorldLocal, EntityId, Pos, Vec) + Send + 'static,
    {
        OnDragStart(HandlerFn::new(move |world, id, (pos, delta)| {
            (fun)(world, id, pos, delta);
            None
        }))
    }
}

impl<R> OnDragStart<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnDragStart<R>
    where
        R: Copy + Send + 'static,
    {
        OnDragStart(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(
        &mut self,
        world: &WorldLocal,
        id: EntityId,
        pos: Pos,
        delta: Vec,
    ) -> Option<R> {
        (self.0).call(world, id, (pos, delta))
    }
}

/// Attach to a widget to react to the moment it stops being the pressed target (see
/// [`crate::ui::handle_event`]) — fires once per release, regardless of where the cursor ends up
/// or whether any [`OnDrag`] events fired in between. See [`OnDragStart`] for the paired
/// press-time trigger.
pub struct OnDragEnd<R = NoAction>(pub Handler<R, (Pos,)>);

impl<R> Component for OnDragEnd<R> where R: 'static {}

impl OnDragEnd {
    #[inline(always)]
    pub fn invoke<F>(mut fun: F) -> OnDragEnd
    where
        F: FnMut(&WorldLocal, EntityId, Pos) + Send + 'static,
    {
        OnDragEnd(HandlerFn::new(move |world, id, (pos,)| {
            (fun)(world, id, pos);
            None
        }))
    }
}

impl<R> OnDragEnd<R> {
    #[inline(always)]
    pub fn emit(signal: R) -> OnDragEnd<R>
    where
        R: Copy + Send + 'static,
    {
        OnDragEnd(emit(signal))
    }

    #[inline(always)]
    pub(crate) fn trigger(&mut self, world: &WorldLocal, id: EntityId, pos: Pos) -> Option<R> {
        (self.0).call(world, id, (pos,))
    }
}
