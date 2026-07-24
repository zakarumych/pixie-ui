//! During layouting and drawing,
//! attributes are calculated for each widget.
//!
//! To calculate the attributes, the theme is used which provides stack of functions
//! that fetch widget-related data and calculate the attributes based on that data.
//! Where later results override earlier results.
//!
//! And finally own widget's instance of `Attributes` is applied to override the calculated attributes.

use edict::{
    component::Component,
    entity::EntityId,
    query::{AsDefaultSendQuery, Entities, QueryItem},
    world::World,
};

use crate::{
    align::Align2,
    draw::{Brush, Stroke},
    font::FontId,
    layout::ContentLayout,
    margin::Margin,
    math::{Pos, Ratio, Size},
    widget::Widget,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Component)]
pub enum WidgetSize {
    Flexible { stretches: (bool, bool) },
    Fixed(Size),
    Relative(Ratio, Ratio),
}

/// Attributes structure that holds various attributes for a widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Component)]
pub struct Attributes {
    /// The position of the widget.
    /// If set, the widget ignores auto layouting and uses this position instead.
    pub position: Option<Pos>,

    /// The size of the widget.
    /// If set, the widget ignores auto layouting and uses this size instead.
    /// If position is set then only content is used to calculate the size of the widget,
    /// otherwise auto layouting is used to calculate both position and size of the widget.
    pub size: Option<WidgetSize>,

    /// The outer margin of the widget.
    /// This affects auto-layouting, ensuring the gap between this widget and its siblings or parent borders.
    pub outer_margin: Option<Margin>,

    /// The alignment of the widget within its parent.
    pub align: Option<Align2>,

    /// The inner margin of the widget.
    /// This affects auto-layouting of the elements of the widget,
    /// ensuring the gap between the widget's border and its elements.
    pub inner_margin: Option<Margin>,

    /// The content layout of the widget.
    /// This affects auto-layouting of the elements of the widget.
    pub content_layout: Option<ContentLayout>,

    /// The default alignment of the widget's elements.
    pub content_align: Option<Align2>,

    /// The brush used to draw the widget's foreground.
    pub fg_brush: Option<Brush>,

    /// The brush used to draw the widget's background.
    pub bg_brush: Option<Brush>,

    /// The stroke used to draw the widget's border.
    pub stroke: Option<Stroke>,

    /// The font used to draw the widget's text.
    pub text_font: Option<FontId>,

    /// The brush used to draw the widget's text.
    pub text_brush: Option<Brush>,
}

impl Attributes {
    pub fn merge(&mut self, other: &Self) {
        if let Some(position) = other.position {
            self.position = Some(position);
        }
        if let Some(size) = other.size {
            self.size = Some(size);
        }
        if let Some(outer_margin) = other.outer_margin {
            self.outer_margin = Some(outer_margin);
        }
        if let Some(align) = other.align {
            self.align = Some(align);
        }
        if let Some(inner_margin) = other.inner_margin {
            self.inner_margin = Some(inner_margin);
        }
        if let Some(content_layout) = other.content_layout {
            self.content_layout = Some(content_layout);
        }
        if let Some(content_align) = other.content_align {
            self.content_align = Some(content_align);
        }
        if let Some(fg_brush) = other.fg_brush {
            self.fg_brush = Some(fg_brush);
        }
        if let Some(bg_brush) = other.bg_brush {
            self.bg_brush = Some(bg_brush);
        }
        if let Some(stroke) = other.stroke {
            self.stroke = Some(stroke);
        }
        if let Some(text_font) = other.text_font {
            self.text_font = Some(text_font);
        }
        if let Some(text_brush) = other.text_brush {
            self.text_brush = Some(text_brush);
        }
    }
}

/// Trait for updating widget attributes.
pub trait AttributesUpdate {
    type Query: AsDefaultSendQuery;

    /// Update the attributes of a widget based on the current state.
    fn update(
        &self,
        attributes: &mut Attributes,
        query: QueryItem<'_, Self::Query>,
        focused: bool,
        hovered: bool,
        pressed: bool,
    );
}

trait AttributesUpdateSystem {
    fn update(&self, world: &mut World, input: InputState);
}

impl<T> AttributesUpdateSystem for T
where
    T: AttributesUpdate,
{
    fn update(&self, world: &mut World, input: InputState) {
        let view = world
            .view::<(Entities, T::Query, &mut ResolvedAttributes)>()
            .with::<Widget>();

        for (e, v, r) in view {
            let focused = input.focused == Some(e.id());
            let hovered = input.hovered == Some(e.id());
            let pressed = input.pressed == Some(e.id());

            self.update(&mut r.0, v, focused, hovered, pressed);
        }
    }
}

/// Style structure that holds a stack of attribute update functions.
pub struct Style {
    pub fallback: Attributes,
    stack: Vec<Box<dyn AttributesUpdateSystem>>,
}

impl Style {
    pub fn new(fallback: Attributes) -> Self {
        Self {
            fallback,
            stack: Vec::new(),
        }
    }

    pub fn push<U: AttributesUpdate + 'static>(&mut self, update: U) {
        self.stack.push(Box::new(update));
    }

    pub(crate) fn resolve_attributes(&self, world: &mut World, input: InputState) {
        let world = world.local();
        let view = world.view::<&mut ResolvedAttributes>().with::<Widget>();
        for r in view {
            r.0 = self.fallback;
        }

        let view = world
            .view::<Entities>()
            .with::<Widget>()
            .without::<ResolvedAttributes>();

        for e in view {
            world.insert_defer(e, ResolvedAttributes(self.fallback));
        }

        for update in &self.stack {
            update.update(world, input);
        }
    }
}

#[doc(hidden)]
pub trait __AttributesUpdateBind<T> {
    type Rest;

    fn bind(self) -> (T, Self::Rest);
}

impl<'a> __AttributesUpdateBind<EntityId> for (EntityId, &'a mut World, InputState) {
    type Rest = (&'a mut World, InputState);

    #[inline(always)]
    fn bind(self) -> (EntityId, (&'a mut World, InputState)) {
        let (widget, world, input) = self;
        (widget, (world, input))
    }
}

impl<'a> __AttributesUpdateBind<&'a mut World> for (EntityId, &'a mut World, InputState) {
    type Rest = (EntityId, InputState);

    #[inline(always)]
    fn bind(self) -> (&'a mut World, (EntityId, InputState)) {
        let (widget, world, input) = self;
        (world, (widget, input))
    }
}

impl<'a> __AttributesUpdateBind<InputState> for (EntityId, &'a mut World, InputState) {
    type Rest = (EntityId, &'a mut World);

    #[inline(always)]
    fn bind(self) -> (InputState, (EntityId, &'a mut World)) {
        let (widget, world, input) = self;
        (input, (widget, world))
    }
}

impl<'a> __AttributesUpdateBind<EntityId> for (EntityId, &'a mut World) {
    type Rest = &'a mut World;

    #[inline(always)]
    fn bind(self) -> (EntityId, &'a mut World) {
        let (widget, world) = self;
        (widget, world)
    }
}

impl<'a> __AttributesUpdateBind<&'a mut World> for (EntityId, &'a mut World) {
    type Rest = EntityId;

    #[inline(always)]
    fn bind(self) -> (&'a mut World, EntityId) {
        let (widget, world) = self;
        (world, widget)
    }
}

impl<'a> __AttributesUpdateBind<&'a mut World> for (&'a mut World, InputState) {
    type Rest = InputState;

    #[inline(always)]
    fn bind(self) -> (&'a mut World, InputState) {
        let (world, input) = self;
        (world, input)
    }
}

impl<'a> __AttributesUpdateBind<InputState> for (&'a mut World, InputState) {
    type Rest = &'a mut World;

    #[inline(always)]
    fn bind(self) -> (InputState, &'a mut World) {
        let (world, input) = self;
        (input, world)
    }
}

impl __AttributesUpdateBind<EntityId> for (EntityId, InputState) {
    type Rest = InputState;

    #[inline(always)]
    fn bind(self) -> (EntityId, InputState) {
        let (widget, input) = self;
        (widget, input)
    }
}

impl __AttributesUpdateBind<InputState> for (EntityId, InputState) {
    type Rest = EntityId;

    #[inline(always)]
    fn bind(self) -> (InputState, EntityId) {
        let (widget, input) = self;
        (input, widget)
    }
}

impl __AttributesUpdateBind<EntityId> for EntityId {
    type Rest = ();

    #[inline(always)]
    fn bind(self) -> (EntityId, ()) {
        (self, ())
    }
}

impl<'a> __AttributesUpdateBind<&'a mut World> for &'a mut World {
    type Rest = ();

    #[inline(always)]
    fn bind(self) -> (&'a mut World, ()) {
        (self, ())
    }
}

impl __AttributesUpdateBind<InputState> for InputState {
    type Rest = ();

    #[inline(always)]
    fn bind(self) -> (InputState, ()) {
        (self, ())
    }
}

/// Defines new [`AttributesUpdate`] implementation
/// using a simple boolean checks and list of attribute updates to apply when all checks pass.
#[macro_export]
macro_rules! attributes_update {
    ($vis:vis $name:ident ($($bind:ident : $bind_type:ty),* $(,)?) if $checks:expr => { $($attr:ident: $value:expr),+ $(,)? }) => {
        $vis struct $name;

        impl $crate::for_macro::AttributesUpdate for $name {
            fn update(
                &self,
                __attributes: &mut $crate::for_macro::Attributes,
                __widget: $crate::for_macro::EntityId,
                __world: &mut $crate::for_macro::World,
                __input: $crate::for_macro::InputState,
            ) {
                let __bindings = (__widget, __world, __input);
                $(
                    let ($bind, __bindings) = $crate::for_macro::__AttributesUpdateBind::<$bind_type>::bind(__bindings);
                    let $bind: $bind_type = $bind;
                )*

                if $checks {
                    $(__attributes.$attr = Some(Into::into($value));)+
                }

                $(
                    let _ = $bind; // silence unused variable warnings
                )*
            }
        }
    };
}

#[derive(Clone, Copy, Default, Component)]
pub(crate) struct ResolvedAttributes(pub Attributes);

#[derive(Clone, Copy, Default)]
pub(crate) struct InputState {
    pub focused: Option<EntityId>,
    pub hovered: Option<EntityId>,
    pub pressed: Option<EntityId>,
}
