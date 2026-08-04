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
    query::{Alt, AsDefaultSendQuery, Entities, QueryItem},
    world::World,
};

use crate::{
    align::{Align, Align2},
    draw::{Brush, Stroke},
    font::FontId,
    layout::{Arranged, ContentLayout},
    margin::Margin,
    math::{Pos, Ratio, Rect, Size, Vec},
    ui::{InputState, Ui},
    widget::Widget,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stretch {
    pub horizontal: bool,
    pub vertical: bool,
}

impl Stretch {
    pub const NONE: Stretch = Stretch {
        horizontal: false,
        vertical: false,
    };

    pub const HORIZONTAL: Stretch = Stretch {
        horizontal: true,
        vertical: false,
    };

    pub const VERTICAL: Stretch = Stretch {
        horizontal: false,
        vertical: true,
    };

    pub const BOTH: Stretch = Stretch {
        horizontal: true,
        vertical: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetSize {
    Flexible(Stretch),
    Fixed(Size),
    Relative(Ratio, Ratio),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl From<Anchor> for Align2 {
    fn from(anchor: Anchor) -> Self {
        match anchor {
            Anchor::TopLeft => Align2 {
                x: Align::Start,
                y: Align::Start,
            },
            Anchor::TopCenter => Align2 {
                x: Align::Center,
                y: Align::Start,
            },
            Anchor::TopRight => Align2 {
                x: Align::End,
                y: Align::Start,
            },
            Anchor::CenterLeft => Align2 {
                x: Align::Start,
                y: Align::Center,
            },
            Anchor::Center => Align2 {
                x: Align::Center,
                y: Align::Center,
            },
            Anchor::CenterRight => Align2 {
                x: Align::End,
                y: Align::Center,
            },
            Anchor::BottomLeft => Align2 {
                x: Align::Start,
                y: Align::End,
            },
            Anchor::BottomCenter => Align2 {
                x: Align::Center,
                y: Align::End,
            },
            Anchor::BottomRight => Align2 {
                x: Align::End,
                y: Align::End,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WidgetPos {
    #[default]
    Auto,
    Fixed {
        offset: Vec,
        anchor: Anchor,
    },
}

/// Attributes structure that holds various attributes for a widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Component)]
pub struct Attributes {
    /// The position of the widget.
    /// If set, the widget ignores auto layouting and uses this position instead.
    pub position: Option<WidgetPos>,

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
    pub font: Option<FontId>,
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
        if let Some(font) = other.font {
            self.font = Some(font);
        }
    }
}

/// Trait for updating widget attributes.
pub trait AttributesUpdate {
    type Query: AsDefaultSendQuery;

    /// Update the attributes of a widget based on the current state.
    ///
    /// `rect` is the widget's own `Arranged.rect` as of the last completed layout pass
    /// (`Rect::ZERO` if the widget hasn't been laid out yet, e.g. its first frame).
    /// `cursor` is the current cursor position, if known.
    /// `dragged` is whether this widget is the pressed target of an in-progress drag.
    fn update(
        &self,
        attributes: &mut Attributes,
        query: QueryItem<'_, Self::Query>,
        rect: Rect,
        cursor: Option<Pos>,
        focused: bool,
        hovered: bool,
        pressed: bool,
        dragged: bool,
    );
}

#[doc(hidden)]
pub trait AttributesUpdateSystem {
    fn update(&self, world: &mut World, input: InputState);
}

impl<T> AttributesUpdateSystem for T
where
    T: AttributesUpdate,
{
    fn update(&self, world: &mut World, input: InputState) {
        let view = world
            .view::<(Entities, T::Query, &mut StyledAttributes, Option<&Arranged>)>()
            .with::<Widget>();

        for (e, v, r, arranged) in view {
            let focused = input.focused == Some(e.id());
            let hovered = input.hovered == Some(e.id());
            let pressed = input.pressed == Some(e.id());
            let dragged = pressed && input.is_dragging;
            let rect = arranged.map_or(Rect::ZERO, |a| a.rect);

            self.update(
                &mut r.0,
                v,
                rect,
                input.cursor,
                focused,
                hovered,
                pressed,
                dragged,
            );
        }
    }
}

/// Style structure that holds a stack of attribute update functions.
pub struct Style {
    pub fallback: Attributes,
    stack: std::vec::Vec<Box<dyn AttributesUpdateSystem>>,
}

impl Style {
    pub fn new(fallback: Attributes) -> Self {
        Self {
            fallback,
            stack: std::vec::Vec::new(),
        }
    }

    pub fn push<U: AttributesUpdate + 'static>(&mut self, update: U) {
        self.stack.push(Box::new(update));
    }

    pub fn resolve_attributes(&self, world: &mut World) {
        let input = {
            let Some(ui) = world.get_resource::<Ui>() else {
                return;
            };

            ui.input()
        };

        let world = world.local();
        let view = world.view::<&mut StyledAttributes>().with::<Widget>();
        for r in view {
            r.0 = self.fallback;
        }

        let view = world
            .view::<Entities>()
            .with::<Widget>()
            .without::<StyledAttributes>();

        for e in view {
            world.insert_defer(e, StyledAttributes(self.fallback));
        }

        let view = world
            .view::<Entities>()
            .with::<Widget>()
            .without::<LayoutAttributes>();

        for e in view {
            world.insert_defer(e, LayoutAttributes::from(&self.fallback));
        }

        let view = world
            .view::<Entities>()
            .with::<Widget>()
            .without::<PaintAttributes>();

        for e in view {
            world.insert_defer(e, PaintAttributes::from(&self.fallback));
        }

        world.run_deferred();

        for update in &self.stack {
            update.update(world, input);
        }

        for (mut layout, mut paint, styled_attrs, own_attrs) in world.view::<(
            Alt<LayoutAttributes>,
            Alt<PaintAttributes>,
            &StyledAttributes,
            Option<&Attributes>,
        )>() {
            let mut attrs = styled_attrs.0;
            if let Some(own_attrs) = own_attrs {
                attrs.merge(own_attrs);
            }

            if !layout.matches(&attrs) {
                *layout = LayoutAttributes::from(&attrs);
            }
            if !paint.matches(&attrs) {
                *paint = PaintAttributes::from(&attrs);
            }
        }
    }
}

/// Defines new [`AttributesUpdate`] implementation
/// using a simple boolean checks and list of attribute updates to apply when all checks pass.
#[macro_export]
macro_rules! attributes_update {
    ($vis:vis $name:ident ($($bind:ident : $bind_type:ty),* $(,)?) if $checks:expr => { $($attr:ident: $value:expr),+ $(,)? }) => {
        $vis struct $name;

        impl $crate::for_macro::AttributesUpdateSystem for $name {
            type Query = ($($bind_type),*);

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
struct StyledAttributes(pub Attributes);

#[derive(Clone, Copy, Default, PartialEq, Eq, Component)]
pub(crate) struct LayoutAttributes {
    /// The position of the widget.
    /// If set, the widget ignores auto layouting and uses this position instead.
    pub position: Option<WidgetPos>,

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

    /// The font used to draw the widget's text.
    pub font: Option<FontId>,
}

impl LayoutAttributes {
    fn from(attrs: &Attributes) -> Self {
        Self {
            position: attrs.position,
            size: attrs.size,
            outer_margin: attrs.outer_margin,
            align: attrs.align,
            inner_margin: attrs.inner_margin,
            content_layout: attrs.content_layout,
            content_align: attrs.content_align,
            font: attrs.font,
        }
    }

    fn matches(&self, attrs: &Attributes) -> bool {
        self.position == attrs.position
            && self.size == attrs.size
            && self.outer_margin == attrs.outer_margin
            && self.align == attrs.align
            && self.inner_margin == attrs.inner_margin
            && self.content_layout == attrs.content_layout
            && self.content_align == attrs.content_align
            && self.font == attrs.font
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Component)]
pub(crate) struct PaintAttributes {
    /// The brush used to draw the widget's foreground.
    pub fg_brush: Option<Brush>,

    /// The brush used to draw the widget's background.
    pub bg_brush: Option<Brush>,

    /// The stroke used to draw the widget's border.
    pub stroke: Option<Stroke>,
}

impl PaintAttributes {
    fn from(attrs: &Attributes) -> Self {
        Self {
            fg_brush: attrs.fg_brush,
            bg_brush: attrs.bg_brush,
            stroke: attrs.stroke,
        }
    }

    fn matches(&self, attrs: &Attributes) -> bool {
        self.fg_brush == attrs.fg_brush
            && self.bg_brush == attrs.bg_brush
            && self.stroke == attrs.stroke
    }
}
