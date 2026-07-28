use edict::{query::With, world::World};
use pixie_ui::{
    align::Align2,
    button::Button,
    color::Color,
    draw::{Brush, Draw, Stroke},
    event::PixieEvent,
    focus::FocusOnClick,
    layout::layout_system,
    margin::Margin,
    math::{Pos, Rect, Size},
    resize::{Resizable, resize_on_drag, resize_on_drag_end, resize_on_drag_start},
    style::{Attributes, AttributesUpdate, Style, WidgetSize},
    text::{Text, TextInput, edit_on_key, edit_on_paste},
    trigger::{OnClick, OnDrag, OnDragEnd, OnDragStart, invoke},
    ui::Ui,
    widget::{Container, RootWidget, SensesClicks, SensesCursor, Widget, sync_widget_parents},
};
use winit::application::ApplicationHandler;

const BUTTON_BASE: Color = Color::from_rgb(70, 110, 200);
const BUTTON_PRESSED: Color = Color::from_rgb(45, 80, 150);
const BUTTON_HOVER: Color = Color::from_rgb(50, 100, 180);

const TEXT_INPUT_STROKE: Color = Color::from_rgb(100, 100, 120);

const SCALE: u32 = 4;

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

struct FocusedInputTextStyle;

impl AttributesUpdate for FocusedInputTextStyle {
    type Query = With<TextInput>;

    fn update(
        &self,
        attributes: &mut Attributes,
        _: (),
        focused: bool,
        _hovered: bool,
        _pressed: bool,
    ) {
        if focused {
            attributes.stroke = Some(Stroke::new(Brush::Solid(TEXT_INPUT_STROKE), 1));
        } else {
            attributes.stroke = None;
        };
    }
}

struct UiState {
    world: World,
    style: Style,
}

impl UiState {
    fn new() -> Self {
        let mut world = World::new();
        world.insert_resource(Ui::new());

        let mut style = Style::new(Attributes::default());
        // style.push(ApplyOwnAttributes);
        style.push(ButtonHoverStyle);
        style.push(FocusedInputTextStyle);

        let title = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    fg_brush: Some(Brush::Solid(Color::WHITE)),
                    // bg_brush: Some(Brush::Solid(Color::from_rgb(10, 10, 12))),
                    outer_margin: Some(Margin::uniform(4)),
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
                    fg_brush: Some(Brush::Solid(Color::from_rgb(200, 200, 210))),
                    // bg_brush: Some(Brush::Solid(Color::from_rgb(10, 10, 12))),
                    outer_margin: Some(Margin::uniform(4)),
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
                    fg_brush: Some(Brush::Solid(Color::WHITE)),
                    content_align: Some(Align2::CENTER),
                    inner_margin: Some(Margin::uniform(8)),
                    outer_margin: Some(Margin::uniform(4)),
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

        let input = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    // position: Some(Pos { x: 16, y: 70 }),
                    // size: Some(WidgetSize::Fixed(Size { w: 250, h: 30 })),
                    fg_brush: Some(Brush::Solid(Color::WHITE)),
                    bg_brush: Some(Brush::Solid(Color::from_rgb(50, 50, 60))),
                    content_align: Some(Align2::LEFT),
                    inner_margin: Some(Margin::uniform(8)),
                    outer_margin: Some(Margin::uniform(4)),
                    ..Default::default()
                },
                Text {
                    string: "Type here".to_string(),
                },
                TextInput::new(),
                SensesCursor,
                SensesClicks,
                FocusOnClick,
                edit_on_key(),
                edit_on_paste(),
            ))
            .id();

        let card = world
            .spawn((
                Widget { parent: None },
                Attributes {
                    position: Some(Pos { x: 32, y: 32 }),
                    size: Some(WidgetSize::Fixed(Size { w: 160, h: 140 })),
                    bg_brush: Some(Brush::Solid(Color::from_rgb(45, 48, 58))),
                    stroke: Some(Stroke::new(Brush::Solid(Color::from_rgb(90, 94, 108)), 1)),
                    inner_margin: Some(Margin::uniform(16)),
                    ..Default::default()
                },
                Resizable::new(Size { w: 8, h: 8 }),
                Container {
                    children: vec![title, counter, button, input],
                },
                SensesCursor,
                SensesClicks,
                OnDragStart(resize_on_drag_start()),
                OnDrag(resize_on_drag()),
                OnDragEnd(resize_on_drag_end()),
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
                inner_margin: Some(Margin::uniform(32)),
                ..Default::default()
            },
            Container {
                children: vec![card],
            },
        ));

        UiState { world, style }
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
                mev::Extent3::new(virt_w * SCALE, virt_h * SCALE, 1),
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
            surface.preferred_present_mode(mev::PresentMode::Mailbox);

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

            pixie_ui::ui::handle_event(world, event);
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
