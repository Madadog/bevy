use crate::{blit::BlitPipeline, upscaling::ViewUpscalingPipeline};
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryState;
use bevy_render::{
    camera::{CameraOutputMode, ExtractedCamera},
    render_graph::{Node, NodeRunError, RenderGraphContext},
    render_resource::{
        BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, LoadOp, Operations,
        PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, SamplerDescriptor,
        TextureViewId,
    },
    renderer::RenderContext,
    view::{ExtractedView, ViewTarget},
};
use std::sync::Mutex;

pub struct UpscalingNode {
    query: QueryState<
        (
            &'static ViewTarget,
            &'static ViewUpscalingPipeline,
            Option<&'static ExtractedCamera>,
        ),
        With<ExtractedView>,
    >,
    cached_texture_bind_group: Mutex<Option<(TextureViewId, BindGroup)>>,
}

impl UpscalingNode {
    pub fn new(world: &mut World) -> Self {
        Self {
            query: QueryState::new(world),
            cached_texture_bind_group: Mutex::new(None),
        }
    }
}

impl Node for UpscalingNode {
    fn update(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph.view_entity();

        let pipeline_cache = world.get_resource::<PipelineCache>().unwrap();
        let blit_pipeline = world.get_resource::<BlitPipeline>().unwrap();

        let (target, upscaling_target, camera) = match self.query.get_manual(world, view_entity) {
            Ok(query) => query,
            Err(_) => return Ok(()),
        };

        let color_attachment_load_op = if let Some(camera) = camera {
            match camera.output_mode {
                CameraOutputMode::Write {
                    color_attachment_load_op,
                    ..
                } => color_attachment_load_op,
                CameraOutputMode::Skip => return Ok(()),
            }
        } else {
            LoadOp::Clear(Default::default())
        };

        let upscaled_texture = target.main_texture();

        let mut cached_bind_group = self.cached_texture_bind_group.lock().unwrap();
        let bind_group = match &mut *cached_bind_group {
            Some((id, bind_group)) if upscaled_texture.id() == *id => bind_group,
            cached_bind_group => {
                let sampler = render_context
                    .render_device()
                    .create_sampler(&SamplerDescriptor::default());

                let bind_group =
                    render_context
                        .render_device()
                        .create_bind_group(&BindGroupDescriptor {
                            label: None,
                            layout: &blit_pipeline.texture_bind_group,
                            entries: &[
                                BindGroupEntry {
                                    binding: 0,
                                    resource: BindingResource::TextureView(upscaled_texture),
                                },
                                BindGroupEntry {
                                    binding: 1,
                                    resource: BindingResource::Sampler(&sampler),
                                },
                            ],
                        });

                let (_, bind_group) = cached_bind_group.insert((upscaled_texture.id(), bind_group));
                bind_group
            }
        };

        let pipeline = match pipeline_cache.get_render_pipeline(upscaling_target.0) {
            Some(pipeline) => pipeline,
            None => return Ok(()),
        };

        let pass_descriptor = RenderPassDescriptor {
            label: Some("upscaling_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.out_texture(),
                resolve_target: None,
                ops: Operations {
                    load: color_attachment_load_op,
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        };

        let mut render_pass = render_context
            .command_encoder()
            .begin_render_pass(&pass_descriptor);

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}
