#[allow(unused_macros)]
macro_rules! for_tuple {
    ($macro:ident) => {
        for_tuple!($macro for A B C D E F G H I J K L);
    };
    ($macro:ident for ) => {
        $macro!();
    };
    ($macro:ident for $a:ident $($b:ident)*) => {
        for_tuple!($macro for $($b)*);
        $macro!($a $($b)*);
    };
}

pub mod align;
pub mod button;
pub mod color;
pub mod draw;
pub mod event;
pub mod focus;
pub mod font;
pub mod layout;
pub mod margin;
pub mod math;
pub mod resize;
pub mod style;
pub mod text;
pub mod texture;
pub mod trigger;
pub mod ui;
pub mod widget;
pub mod world;

#[doc(hidden)]
pub mod for_macro {
    pub use crate::style::{Attributes, AttributesUpdate, AttributesUpdateSystem};
    pub use edict::{entity::EntityId, world::World};
}
