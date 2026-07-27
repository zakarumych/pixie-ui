use std::collections::HashMap;

use mev::Arguments as _;
use pixie_ui::{
    draw::{AddressMode, AddressMode2, Draw},
    font::FontId,
    math::Rect,
    texture::TextureId,
    ui::Ui,
};

use crate::{
    Error,
    atlas::{self, Atlas},
    brush::resolve_brush,
    gpu::{Globals, GlyphInstanceGpu, RectArguments, RectInstanceGpu, TextArguments},
};

fn to_mev_address(mode: AddressMode) -> mev::AddressMode {
    match mode {
        AddressMode::Repeat => mev::AddressMode::Repeat,
        AddressMode::Mirrored => mev::AddressMode::MirrorRepeat,
        AddressMode::Edge => mev::AddressMode::ClampToEdge,
    }
}

fn image_size(image: &mev::Image) -> (f32, f32) {
    let extent = image.extent().into_2d();
    (extent.width() as f32, extent.height() as f32)
}

fn rect_to_vec4(r: Rect) -> mev::vec4<f32> {
    mev::vec4(r.lt.x as f32, r.lt.y as f32, r.rb.x as f32, r.rb.y as f32)
}

fn expand_rect(r: Rect, amount: f32) -> mev::vec4<f32> {
    mev::vec4(
        r.lt.x as f32 - amount,
        r.lt.y as f32 - amount,
        r.rb.x as f32 + amount,
        r.rb.y as f32 + amount,
    )
}

/// A single texture/sampler binding required to draw a batch of instances.
#[derive(Clone, Copy)]
struct Binding {
    texture: Option<TextureId>,
    mode: AddressMode2,
}

impl Default for Binding {
    fn default() -> Self {
        Binding {
            texture: None,
            mode: AddressMode2::from(AddressMode::Edge),
        }
    }
}

enum Layer {
    Rect {
        index: u32,
        binding: Binding,
    },
    Text {
        first: u32,
        count: u32,
        binding: Binding,
        font: FontId,
    },
}

/// Renders pixie-ui `Draw` commands onto an existing `mev::Image`.
pub struct Renderer {
    device: mev::Device,
    rect_pipeline: Option<(mev::PixelFormat, mev::RenderPipeline)>,
    text_pipeline: Option<(mev::PixelFormat, mev::RenderPipeline)>,

    textures: HashMap<TextureId, mev::Image>,
    samplers: HashMap<AddressMode2, mev::Sampler>,
    atlases: HashMap<FontId, Atlas>,

    dummy_image: mev::Image,
    dummy_sampler: mev::Sampler,
    atlas_sampler: mev::Sampler,

    globals_buffer: mev::Buffer,
    rect_buffer: Option<mev::Buffer>,
    text_buffer: Option<mev::Buffer>,
}

impl Renderer {
    /// Creates a new renderer, uploading the small fixed resources (dummy texture,
    /// samplers, globals buffer) synchronously.
    pub fn new(queue: &mut mev::Queue) -> Result<Self, Error> {
        let dummy_image = queue.new_image(mev::ImageDesc::new_d2_texture(
            1,
            1,
            mev::PixelFormat::Rgba8Unorm,
        ));
        let staging = queue.new_buffer_init(mev::BufferInitDesc {
            data: &[255u8, 255, 255, 255],
            usage: mev::BufferUsage::TRANSFER_SRC,
            name: "pixie-ui-mev-dummy-staging",
        });

        let mut encoder = queue.new_command_encoder();
        encoder.init_image(
            mev::PipelineStages::empty(),
            mev::PipelineStages::TRANSFER,
            &dummy_image,
        );
        {
            let mut copy = encoder.copy();
            copy.copy_buffer_to_image(
                &staging,
                0,
                4,
                4,
                &dummy_image,
                mev::Offset3::ZERO,
                mev::Extent3::new(1, 1, 1),
                0..1,
                0,
            );
            copy.barrier(
                mev::PipelineStages::TRANSFER,
                mev::PipelineStages::FRAGMENT_SHADER,
            );
        }
        let cbuf = encoder.finish();
        queue.submit_checkpoint([cbuf])?;
        queue.wait_idle()?;

        let dummy_sampler = queue.new_sampler(mev::SamplerDesc::new());
        let atlas_sampler = queue.new_sampler(mev::SamplerDesc {
            address_mode: [mev::AddressMode::ClampToEdge; 3],
            ..mev::SamplerDesc::new()
        });

        let globals_buffer = queue.new_buffer(mev::BufferDesc {
            size: std::mem::size_of::<Globals>(),
            usage: mev::BufferUsage::UNIFORM | mev::BufferUsage::TRANSFER_DST,
            name: "pixie-ui-mev-globals",
        });

        Ok(Renderer {
            device: queue.device().clone(),
            rect_pipeline: None,
            text_pipeline: None,
            textures: HashMap::new(),
            samplers: HashMap::new(),
            atlases: HashMap::new(),
            dummy_image,
            dummy_sampler,
            atlas_sampler,
            globals_buffer,
            rect_buffer: None,
            text_buffer: None,
        })
    }

    /// Registers a `mev::Image` to be used for `Brush::Texture` draws referencing `id`.
    pub fn insert_texture(&mut self, id: TextureId, image: mev::Image) {
        self.textures.insert(id, image);
    }

    /// Removes a previously registered texture.
    pub fn remove_texture(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    fn sampler_for(&mut self, mode: AddressMode2) -> mev::Sampler {
        if let Some(sampler) = self.samplers.get(&mode) {
            return sampler.clone();
        }
        let desc = mev::SamplerDesc {
            address_mode: [
                to_mev_address(mode.x),
                to_mev_address(mode.y),
                mev::AddressMode::ClampToEdge,
            ],
            ..mev::SamplerDesc::new()
        };
        let sampler = self.device.new_sampler(desc);
        self.samplers.insert(mode, sampler.clone());
        sampler
    }

    fn ensure_atlas(
        &mut self,
        queue: &mut mev::Queue,
        font_id: FontId,
        ui: &Ui,
    ) -> Result<bool, Error> {
        if self.atlases.contains_key(&font_id) {
            return Ok(true);
        }
        let Some(font) = ui.font(font_id) else {
            return Ok(false);
        };
        let atlas = atlas::build_atlas(queue, font)?;
        self.atlases.insert(font_id, atlas);
        Ok(true)
    }

    fn ensure_rect_pipeline(&mut self, format: mev::PixelFormat) -> Result<(), Error> {
        if matches!(&self.rect_pipeline, Some((f, _)) if *f == format) {
            return Ok(());
        }
        let pipeline = build_pipeline(
            &mut self.device,
            crate::gpu::RECT_WGSL,
            format,
            &[RectArguments::LAYOUT],
        )?;
        self.rect_pipeline = Some((format, pipeline));
        Ok(())
    }

    fn ensure_text_pipeline(&mut self, format: mev::PixelFormat) -> Result<(), Error> {
        if matches!(&self.text_pipeline, Some((f, _)) if *f == format) {
            return Ok(());
        }
        let pipeline = build_pipeline(
            &mut self.device,
            crate::gpu::TEXT_WGSL,
            format,
            &[TextArguments::LAYOUT],
        )?;
        self.text_pipeline = Some((format, pipeline));
        Ok(())
    }

    fn ensure_rect_buffer(&mut self, count: usize) -> Result<(), Error> {
        let needed = count.max(1) * std::mem::size_of::<RectInstanceGpu>();
        let recreate = match &self.rect_buffer {
            Some(buffer) => buffer.size() < needed,
            None => true,
        };
        if recreate {
            let size = needed.next_power_of_two();
            self.rect_buffer = Some(self.device.new_buffer(mev::BufferDesc {
                size,
                usage: mev::BufferUsage::STORAGE | mev::BufferUsage::TRANSFER_DST,
                name: "pixie-ui-mev-rect-instances",
            }));
        }
        Ok(())
    }

    fn ensure_text_buffer(&mut self, count: usize) -> Result<(), Error> {
        let needed = count.max(1) * std::mem::size_of::<GlyphInstanceGpu>();
        let recreate = match &self.text_buffer {
            Some(buffer) => buffer.size() < needed,
            None => true,
        };
        if recreate {
            let size = needed.next_power_of_two();
            self.text_buffer = Some(self.device.new_buffer(mev::BufferDesc {
                size,
                usage: mev::BufferUsage::STORAGE | mev::BufferUsage::TRANSFER_DST,
                name: "pixie-ui-mev-text-instances",
            }));
        }
        Ok(())
    }

    /// Draws `draws` onto `image`, compositing over its existing content (it is not cleared).
    /// Submits synchronously: the GPU work is complete by the time this call returns.
    pub fn render(
        &mut self,
        queue: &mut mev::Queue,
        image: &mev::Image,
        draws: &[Draw],
        ui: &Ui,
    ) -> Result<(), Error> {
        let format = image.format();
        let extent = image.extent().into_2d();
        let width = extent.width().max(1);
        let height = extent.height().max(1);

        self.ensure_rect_pipeline(format)?;
        self.ensure_text_pipeline(format)?;

        let mut rect_instances: Vec<RectInstanceGpu> = Vec::new();
        let mut text_instances: Vec<GlyphInstanceGpu> = Vec::new();
        let mut layers: Vec<Layer> = Vec::new();

        for draw in draws {
            match draw {
                Draw::Rect {
                    geometry,
                    fill,
                    stroke,
                } => {
                    if let Some(brush) = fill {
                        let resolved = resolve_brush(brush, *geometry, |id| {
                            self.textures.get(&id).map(image_size)
                        });
                        let index = rect_instances.len() as u32;
                        rect_instances.push(RectInstanceGpu {
                            geom0: rect_to_vec4(*geometry),
                            geom1: rect_to_vec4(*geometry),
                            flags: mev::vec4(0u32, resolved.kind, 0, 0),
                            color0: mev::vec4(
                                resolved.color0[0],
                                resolved.color0[1],
                                resolved.color0[2],
                                resolved.color0[3],
                            ),
                            color1: mev::vec4(
                                resolved.color1[0],
                                resolved.color1[1],
                                resolved.color1[2],
                                resolved.color1[3],
                            ),
                            grad: mev::vec4(
                                resolved.grad_start[0],
                                resolved.grad_start[1],
                                resolved.grad_end[0],
                                resolved.grad_end[1],
                            ),
                            uvparam: mev::vec4(
                                resolved.uv_scale[0],
                                resolved.uv_scale[1],
                                resolved.uv_bias[0],
                                resolved.uv_bias[1],
                            ),
                        });
                        let binding = match resolved.texture {
                            Some((texture, mode)) => Binding {
                                texture: Some(texture),
                                mode,
                            },
                            None => Binding::default(),
                        };
                        layers.push(Layer::Rect { index, binding });
                    }

                    if let Some(stroke) = stroke {
                        let outer_off = stroke.offset as f32 + stroke.width as f32;
                        let inner_off = stroke.offset as f32;
                        let resolved = resolve_brush(&stroke.brush, *geometry, |id| {
                            self.textures.get(&id).map(image_size)
                        });
                        let index = rect_instances.len() as u32;
                        rect_instances.push(RectInstanceGpu {
                            geom0: expand_rect(*geometry, outer_off),
                            geom1: expand_rect(*geometry, inner_off),
                            flags: mev::vec4(1u32, resolved.kind, 0, 0),
                            color0: mev::vec4(
                                resolved.color0[0],
                                resolved.color0[1],
                                resolved.color0[2],
                                resolved.color0[3],
                            ),
                            color1: mev::vec4(
                                resolved.color1[0],
                                resolved.color1[1],
                                resolved.color1[2],
                                resolved.color1[3],
                            ),
                            grad: mev::vec4(
                                resolved.grad_start[0],
                                resolved.grad_start[1],
                                resolved.grad_end[0],
                                resolved.grad_end[1],
                            ),
                            uvparam: mev::vec4(
                                resolved.uv_scale[0],
                                resolved.uv_scale[1],
                                resolved.uv_bias[0],
                                resolved.uv_bias[1],
                            ),
                        });
                        let binding = match resolved.texture {
                            Some((texture, mode)) => Binding {
                                texture: Some(texture),
                                mode,
                            },
                            None => Binding::default(),
                        };
                        layers.push(Layer::Rect { index, binding });
                    }
                }
                Draw::Text {
                    rect,
                    font,
                    glyphs,
                    brush,
                } => {
                    if glyphs.is_empty() {
                        continue;
                    }
                    if !self.ensure_atlas(queue, *font, ui)? {
                        continue;
                    }
                    let Some(font_data) = ui.font(*font) else {
                        continue;
                    };

                    let resolved =
                        resolve_brush(brush, *rect, |id| self.textures.get(&id).map(image_size));

                    let first = text_instances.len() as u32;
                    let mut cursor_x = rect.lt.x as f32;
                    let cursor_y = rect.lt.y as f32;

                    // Borrow ends before we push into `layers` / call `self.ensure_atlas` again.
                    let atlas = self.atlases.get(font).expect("atlas ensured above");

                    for glyph in glyphs.iter() {
                        let Some(metrics) = font_data.glyph_metrics.get(glyph.0 as usize) else {
                            continue;
                        };
                        let bbox = metrics.bbox;
                        let vis_w = (bbox.rb.x - bbox.lt.x).max(0) as u32;
                        let vis_h = (bbox.rb.y - bbox.lt.y).max(0) as u32;
                        let (u0, v0, u1, v1) = atlas.glyph_uv(
                            glyph.0,
                            bbox.lt.x.max(0) as u32,
                            bbox.lt.y.max(0) as u32,
                            vis_w,
                            vis_h,
                        );

                        let pen_x = cursor_x + metrics.offset.x as f32;
                        let pen_y = cursor_y + metrics.offset.y as f32;

                        text_instances.push(GlyphInstanceGpu {
                            geom: mev::vec4(
                                pen_x,
                                pen_y,
                                pen_x + vis_w as f32,
                                pen_y + vis_h as f32,
                            ),
                            atlas_uv: mev::vec4(u0, v0, u1, v1),
                            flags: mev::vec4(resolved.kind, 0, 0, 0),
                            color0: mev::vec4(
                                resolved.color0[0],
                                resolved.color0[1],
                                resolved.color0[2],
                                resolved.color0[3],
                            ),
                            color1: mev::vec4(
                                resolved.color1[0],
                                resolved.color1[1],
                                resolved.color1[2],
                                resolved.color1[3],
                            ),
                            grad: mev::vec4(
                                resolved.grad_start[0],
                                resolved.grad_start[1],
                                resolved.grad_end[0],
                                resolved.grad_end[1],
                            ),
                            uvparam: mev::vec4(
                                resolved.uv_scale[0],
                                resolved.uv_scale[1],
                                resolved.uv_bias[0],
                                resolved.uv_bias[1],
                            ),
                        });

                        cursor_x += metrics.advance.w as f32;
                    }

                    let count = text_instances.len() as u32 - first;
                    if count > 0 {
                        let binding = match resolved.texture {
                            Some((texture, mode)) => Binding {
                                texture: Some(texture),
                                mode,
                            },
                            None => Binding::default(),
                        };
                        layers.push(Layer::Text {
                            first,
                            count,
                            binding,
                            font: *font,
                        });
                    }
                }
            }
        }

        if layers.is_empty() {
            return Ok(());
        }

        self.ensure_rect_buffer(rect_instances.len())?;
        self.ensure_text_buffer(text_instances.len())?;

        let mut encoder = queue.new_command_encoder();
        {
            let mut copy = encoder.copy();
            copy.barrier(
                mev::PipelineStages::VERTEX_SHADER | mev::PipelineStages::FRAGMENT_SHADER,
                mev::PipelineStages::TRANSFER,
            );
            copy.write_buffer(&self.globals_buffer, &Globals { width, height });
            if !rect_instances.is_empty() {
                copy.write_buffer_slice(self.rect_buffer.as_ref().unwrap(), &rect_instances);
            }
            if !text_instances.is_empty() {
                copy.write_buffer_slice(self.text_buffer.as_ref().unwrap(), &text_instances);
            }
            copy.barrier(
                mev::PipelineStages::TRANSFER,
                mev::PipelineStages::VERTEX_SHADER | mev::PipelineStages::FRAGMENT_SHADER,
            );
        }

        {
            let mut render = encoder.render(mev::RenderPassDesc {
                name: "pixie-ui-mev",
                color_attachments: &[mev::AttachmentDesc::new(image)],
                depth_stencil_attachment: None,
            });

            render.with_viewport(
                mev::Offset3::ZERO,
                mev::Extent3::new(width, height, 1).cast_as_f32(),
            );
            render.with_scissor(mev::Offset2::ZERO, mev::Extent2::new(width, height));

            for layer in &layers {
                match layer {
                    Layer::Rect { index, binding } => {
                        let tex_image = match binding.texture {
                            Some(id) => self
                                .textures
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| self.dummy_image.clone()),
                            None => self.dummy_image.clone(),
                        };
                        let sampler = match binding.texture {
                            Some(_) => self.sampler_for(binding.mode),
                            None => self.dummy_sampler.clone(),
                        };
                        let args = RectArguments {
                            globals: self.globals_buffer.clone(),
                            instances: self.rect_buffer.clone().unwrap(),
                            tex: tex_image,
                            samp: sampler,
                        };

                        render.with_pipeline(&self.rect_pipeline.as_ref().unwrap().1);
                        render.with_arguments(0, &args);
                        render.draw(0..6, *index..*index + 1);
                    }
                    Layer::Text {
                        first,
                        count,
                        binding,
                        font,
                    } => {
                        let atlas_image = self.atlases.get(font).unwrap().image.clone();
                        let tex_image = match binding.texture {
                            Some(id) => self
                                .textures
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| self.dummy_image.clone()),
                            None => self.dummy_image.clone(),
                        };
                        let sampler = match binding.texture {
                            Some(_) => self.sampler_for(binding.mode),
                            None => self.dummy_sampler.clone(),
                        };
                        let args = TextArguments {
                            globals: self.globals_buffer.clone(),
                            instances: self.text_buffer.clone().unwrap(),
                            atlas: atlas_image,
                            atlas_samp: self.atlas_sampler.clone(),
                            tex: tex_image,
                            samp: sampler,
                        };

                        render.with_pipeline(&self.text_pipeline.as_ref().unwrap().1);
                        render.with_arguments(0, &args);
                        render.draw(0..6, *first..*first + *count);
                    }
                }
            }
        }

        let cbuf = encoder.finish();
        queue.submit([cbuf])?;

        Ok(())
    }
}

fn build_pipeline(
    device: &mut mev::Device,
    wgsl: &str,
    format: mev::PixelFormat,
    arguments: &[mev::ArgumentGroupLayout<'static>],
) -> Result<mev::RenderPipeline, Error> {
    let library = device.new_shader_library(mev::LibraryDesc {
        name: "pixie-ui-mev",
        input: mev::LibraryInput::wgsl(wgsl),
    })?;

    let pipeline = device.new_render_pipeline(mev::RenderPipelineDesc {
        name: "pixie-ui-mev",
        vertex_shader: mev::Shader {
            library: library.clone(),
            entry: "vs_main".into(),
        },
        vertex_layouts: vec![],
        vertex_attributes: vec![],
        primitive_topology: mev::PrimitiveTopology::Triangle,
        raster: Some(mev::RasterDesc {
            fragment_shader: Some(mev::Shader {
                library,
                entry: "fs_main".into(),
            }),
            color_targets: vec![mev::ColorTargetDesc {
                format,
                blend: Some(mev::BlendDesc::default()),
            }],
            depth_stencil: None,
            front_face: mev::FrontFace::default(),
            culling: mev::Culling::None,
        }),
        arguments,
        constants: 0,
    })?;

    Ok(pipeline)
}
