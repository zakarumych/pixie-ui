use edict::{entity::EntityId, query::With, world::World};
use pixie_ui::{
    align::Align,
    button::Button,
    color::Color,
    draw::{Brush, Draw},
    event::PixieEvent,
    layout::layout_system,
    math::{Pos, Rect, Size},
    style::{Attributes, AttributesUpdate, Style, WidgetSize},
    text::Text,
    trigger::{OnClick, invoke},
    ui::Ui,
    widget::{Container, RootWidget, SensesClicks, SensesCursor, Widget, sync_widget_parents},
};
use winit::application::ApplicationHandler;

const CARD_SIZE: Size = Size { w: 280, h: 170 };
const BUTTON_LOCAL_POS: Pos = Pos { x: 16, y: 110 };
const BUTTON_SIZE: Size = Size { w: 120, h: 34 };

const BUTTON_BASE: Color = Color::from_rgb(70, 110, 200);
const BUTTON_PRESSED: Color = Color::from_rgb(45, 80, 150);
const BUTTON_HOVER: Color = Color::from_rgb(50, 100, 180);

const SCALE: u32 = 4;

fn card_pos(virt_w: u32, virt_h: u32) -> Pos {
    let x = ((virt_w as i32) - CARD_SIZE.w as i32).max(0) / 2;
    let y = ((virt_h as i32) - CARD_SIZE.h as i32).max(0) / 2;
    Pos { x, y }
}

struct ButtonHoverStyle;

impl AttributesUpdate for ButtonHoverStyle {
    type Query = With<Button>;

    fn update(
        &self,
        attributes: &mut Attributes,
        _: (),
        _focused: bool,
        hovered: bool,
        pressed: bool,
    ) {
        if pressed {
            attributes.bg_brush = Some(Brush::Solid(BUTTON_PRESSED));
        } else if hovered {
            attributes.bg_brush = Some(Brush::Solid(BUTTON_HOVER));
        } else {
            attributes.bg_brush = Some(Brush::Solid(BUTTON_BASE));
        };
    }
}

struct UiState {
    world: World,
    style: Style,
    card: EntityId,
}

impl UiState {
    fn new() -> Self {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let mut style = Style::new(Attributes::default());
        // style.push(ApplyOwnAttributes);
        style.push(ButtonHoverStyle);

        // let text = world
        //     .spawn((
        //         Widget { parent: None },
        //         Text {
        //             string: "Click me".to_string(),
        //         },
        //         Attributes {
        //             bg_brush: Some(Brush::Solid(Color::TRANSPARENT)),
        //             ..Default::default()
        //         },
        //     ))
        //     .id();

        let title = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    position: Some(Pos { x: 16, y: 14 }),
                    text_brush: Some(Brush::Solid(Color::WHITE)),
                    ..Default::default()
                },
                Text {
                    string: "pixie-ui demo".to_string(),
                },
            ))
            .id();

        let counter = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    position: Some(Pos { x: 16, y: 36 }),
                    text_brush: Some(Brush::Solid(Color::from_rgb(200, 200, 210))),
                    ..Default::default()
                },
                Text {
                    string: "Clicks: 0".to_string(),
                },
            ))
            .id();

        let mut clicks = 0;

        let button = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    position: Some(BUTTON_LOCAL_POS),
                    size: Some(WidgetSize::Fixed(BUTTON_SIZE)),
                    text_brush: Some(Brush::Solid(Color::WHITE)),
                    text_align: Some(Align::Center.into()),
                    // text_font: Some(FontId(1)),
                    content_align: Some(Align::Center.into()),
                    ..Default::default()
                },
                Button,
                Text {
                    string: "Click me".to_string(),
                },
                SensesCursor,
                SensesClicks,
                // OnClick(emit(Action::Increment)),
                OnClick(invoke(move |world, _| {
                    let Ok(mut view) = world.try_view_one::<&mut Text>(counter) else {
                        return;
                    };
                    let Some(text) = view.get_mut() else {
                        return;
                    };
                    text.string = format!("Clicks: {}", clicks + 1);
                    clicks += 1;
                })),
            ))
            .id();

        let card = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    position: Some(Pos { x: 0, y: 0 }),
                    size: Some(WidgetSize::Fixed(CARD_SIZE)),
                    bg_brush: Some(Brush::Solid(Color::from_rgb(45, 48, 58))),
                    ..Default::default()
                },
                Container {
                    children: vec![title, counter, button],
                },
            ))
            .id();

        world.spawn((
            Widget { parent: None },
            RootWidget,
            Attributes {
                size: Some(WidgetSize::Flexible {
                    stretches: (true, true),
                }),
                bg_brush: Some(Brush::Solid(Color::from_rgb(18, 18, 24))),
                ..Default::default()
            },
            Container {
                children: vec![card],
            },
        ));

        UiState { world, style, card }
    }

    fn set_card_pos(&mut self, pos: Pos) {
        if let Ok(attrs) = self.world.get::<&mut Attributes>(self.card) {
            attrs.position = Some(pos);
        }
    }
}

struct App {
    renderer: Option<pixie_ui_mev::Renderer>,
    ui_state: UiState,
    queue: Option<mev::Queue>,
    surface: Option<mev::Surface>,
    window: Option<winit::window::Window>,
    offscreen: Option<mev::Image>,
    offscreen_size: (u32, u32),
    blit: Option<mev::kernels::blit::Blit>,
    device: Option<mev::Device>,
}

impl App {
    fn render(&mut self) {
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_mut().unwrap();
        let window = self.window.as_ref().unwrap();

        let size = window.inner_size();
        let virt_w = ((size.width + SCALE - 1) / SCALE).max(1);
        let virt_h = ((size.height + SCALE - 1) / SCALE).max(1);

        if self.offscreen_size != (virt_w, virt_h) {
            let image = device.new_image(mev::ImageDesc::new_d2_rt(
                virt_w,
                virt_h,
                mev::PixelFormat::Rgba8Unorm,
            ));
            self.offscreen = Some(image);
            self.offscreen_size = (virt_w, virt_h);
        }

        let cp = card_pos(virt_w, virt_h);
        self.ui_state.set_card_pos(cp);

        sync_widget_parents(&mut self.ui_state.world);

        self.ui_state
            .style
            .resolve_attributes(&mut self.ui_state.world);

        layout_system(&mut self.ui_state.world);

        {
            let mut ui = self
                .ui_state
                .world
                .get_resource_mut::<Ui>()
                .expect("Ui resource missing");
            ui.set_rect(Rect {
                lt: Pos::ZERO,
                rb: Pos {
                    x: virt_w as i32,
                    y: virt_h as i32,
                },
            });
        }

        let mut draws: std::vec::Vec<Draw> = std::vec::Vec::new();
        Ui::draw_ui(&mut self.ui_state.world, &mut draws);

        let offscreen = self.offscreen.as_ref().unwrap().clone();

        {
            let mut encoder = queue.new_command_encoder();
            encoder.init_image(
                mev::PipelineStages::empty(),
                mev::PipelineStages::COLOR_OUTPUT,
                &offscreen,
            );
            let cbuf = encoder.finish();
            queue.submit([cbuf]).unwrap();
        }

        self.renderer
            .as_mut()
            .unwrap()
            .render(
                queue,
                &offscreen,
                &draws,
                &self.ui_state.world.get_resource::<Ui>().unwrap(),
            )
            .unwrap();

        let mut frame = self.surface.as_mut().unwrap().next_frame().unwrap();

        if self.blit.is_none() {
            self.blit = Some(mev::kernels::blit::Blit::new(device));
        }

        {
            queue.sync_frame(&mut frame, mev::PipelineStages::COLOR_OUTPUT);

            let mut encoder = queue.new_command_encoder();
            encoder.init_image(
                mev::PipelineStages::empty(),
                mev::PipelineStages::COLOR_OUTPUT,
                frame.image(),
            );

            // Make the offscreen UI render (already complete — `Renderer::render` submits and
            // waits idle synchronously) visible to the blit's fragment shader read below.
            encoder.barrier(
                mev::PipelineStages::COLOR_OUTPUT,
                mev::PipelineStages::FRAGMENT_SHADER,
            );

            self.blit.as_ref().unwrap().blit(
                &mut encoder,
                &offscreen,
                mev::Offset3::ZERO,
                mev::Extent3::new(virt_w, virt_h, 1),
                frame.image(),
                mev::Offset3::ZERO,
                frame.image().extent().into_3d(),
                [mev::AddressMode::Repeat; 3],
                mev::Filter::Nearest,
            );

            window.pre_present_notify();
            encoder.present(frame, mev::PipelineStages::COLOR_OUTPUT);
            let cbuf = encoder.finish();
            queue.submit_checkpoint([cbuf]).unwrap();
        }
    }
}

enum Action {}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = winit::window::WindowAttributes::default()
                .with_title("pixie-ui demo")
                .with_inner_size(winit::dpi::LogicalSize::new(800, 600));

            let window = event_loop.create_window(attributes).unwrap();

            let device = self.device.as_ref().unwrap();
            let mut surface = device.new_surface(&window, &window).unwrap();

            let size = window.inner_size();
            surface.preferred_extent(mev::Extent2::new(size.width, size.height));
            surface.preferred_usage(mev::ImageUsage::TARGET);
            surface.preferred_present_mode(mev::PresentMode::Fifo);

            self.window = Some(window);
            self.surface = Some(surface);

            if self.renderer.is_none() {
                let queue = self.queue.as_mut().unwrap();
                self.renderer = Some(pixie_ui_mev::Renderer::new(queue).unwrap());
            }
        }

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let Some(mut event) = pixie_ui_winit::convert_event(&event) {
            // `convert_event` reports physical window pixels; widget layout/hit-testing
            // happens in virtual (pre-upscale) pixels, so cursor positions need the same
            // `/ SCALE` the render path applies to the window size.
            if let PixieEvent::CursorMoved { pos } = &mut event {
                pos.x /= SCALE as i32;
                pos.y /= SCALE as i32;
            }

            let world = &mut self.ui_state.world;

            for action in pixie_ui::ui::handle_event::<Action>(world, event) {
                match action {}
            }
        }

        match event {
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.preferred_extent(mev::Extent2::new(size.width, size.height));
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.render();
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    let instance = mev::Instance::load().expect("Failed to init graphics");

    let (device, mut queues) = instance
        .new_device(mev::DeviceDesc {
            idx: 0,
            queues: &[0],
            features: mev::Features::SURFACE,
        })
        .unwrap();
    let queue = queues.pop().unwrap();

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        surface: None,
        queue: Some(queue),
        device: Some(device),
        renderer: None,
        ui_state: UiState::new(),
        offscreen: None,
        offscreen_size: (0, 0),
        blit: None,
    };

    let _ = event_loop.run_app(&mut app);
}
