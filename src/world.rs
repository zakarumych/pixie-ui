use edict::world::World;

use crate::{
    draw::{Draw, draw_ui},
    event::PixieEvent,
    trigger::NoAction,
};

pub trait WorldExt {
    fn draw_ui<'a>(&mut self, commands: &mut impl Extend<Draw<'a>>);

    fn handle_event(&mut self, event: PixieEvent);

    fn handle_event_with_actions<A: 'static>(
        &mut self,
        event: PixieEvent,
    ) -> impl Iterator<Item = A>;
}

impl WorldExt for World {
    #[inline(always)]
    fn draw_ui<'a>(&mut self, commands: &mut impl Extend<Draw<'a>>) {
        draw_ui(self, commands);
    }

    #[inline]
    fn handle_event(&mut self, event: PixieEvent) {
        for a in self.handle_event_with_actions::<NoAction>(event) {
            match a {}
        }
    }

    #[inline(always)]
    fn handle_event_with_actions<A: 'static>(
        &mut self,
        event: PixieEvent,
    ) -> impl Iterator<Item = A> {
        crate::event::handle_event_with_actions(self, event)
    }
}
