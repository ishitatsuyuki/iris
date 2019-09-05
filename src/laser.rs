use amethyst::core::{
    ecs::{Component, DenseVecStorage, Entities, Join, ReadExpect, ReadStorage, SystemData, World},
    math::{Matrix4, Point3, Vector3},
    transform::{ParentHierarchy, Transform},
};
use amethyst::renderer::{
    bundle::{RenderOrder, RenderPlan, RenderPlugin, Target},
    palette::rgb::LinSrgb,
    pass::validate_spirv,
    pipeline::{PipelineDescBuilder, PipelinesBuilder},
    pod::VertexArgs,
    rendy::{
        command::{QueueId, RenderPassEncoder},
        factory::Factory,
        graph::{
            render::{PrepareResult, RenderGroup, RenderGroupDesc},
            GraphContext, NodeBuffer, NodeImage,
        },
        hal::{device::Device, pass::Subpass, pso},
        mesh::{AsVertex, Mesh, MeshBuilder, PosTex},
        shader::{ShaderSetBuilder, SpirvShader},
    },
    submodules::{DynamicUniform, DynamicVertexBuffer, EnvironmentSub},
    types::Backend,
};
use failure::Error;
use glsl_layout::*;
use std::iter;
use std::marker::PhantomData;

pub struct Laser {
    pub color: LinSrgb<f32>,
}

impl Component for Laser {
    type Storage = DenseVecStorage<Self>;
}

#[derive(Debug)]
pub struct LaserOptions {
    pub basis: Point3<f32>,
    pub judge_quad: Vec<Point3<f32>>,
}

impl Default for LaserOptions {
    fn default() -> Self {
        Self {
            basis: Point3::new(0., 0., 0.),
            judge_quad: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, AsStd140)]
struct LaserArgs {
    basis: vec3,
    pre_transform: mat4,
    post_transform: mat4,
}

pub struct Note {}

impl Component for Note {
    type Storage = DenseVecStorage<Self>;
}

lazy_static::lazy_static! {
    static ref LASER_VERTEX: SpirvShader = SpirvShader::new(
        validate_spirv(include_bytes!("../compiled/vertex/laser.vert.spv")),
        pso::ShaderStageFlags::VERTEX,
        "main",
    );

    static ref LASER_FRAGMENT: SpirvShader = SpirvShader::new(
        validate_spirv(include_bytes!("../compiled/fragment/laser.frag.spv")),
        pso::ShaderStageFlags::FRAGMENT,
        "main",
    );

    static ref LASER_SHADERS: ShaderSetBuilder = ShaderSetBuilder::default()
        .with_vertex(&*LASER_VERTEX).unwrap()
        .with_fragment(&*LASER_FRAGMENT).unwrap();
}

#[derive(Clone, Debug, Default)]
pub struct DrawLaserDesc<B: Backend> {
    marker: PhantomData<B>,
}

impl<B: Backend> DrawLaserDesc<B> {
    pub fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<B: Backend> RenderGroupDesc<B, World> for DrawLaserDesc<B> {
    fn build(
        self,
        _: &GraphContext<B>,
        factory: &mut Factory<B>,
        queue: QueueId,
        _: &World,
        framebuffer_width: u32,
        framebuffer_height: u32,
        subpass: Subpass<B>,
        _: Vec<NodeBuffer>,
        _: Vec<NodeImage>,
    ) -> Result<Box<dyn RenderGroup<B, World>>, Error> {
        let env = EnvironmentSub::new(
            factory,
            [
                pso::ShaderStageFlags::VERTEX,
                pso::ShaderStageFlags::FRAGMENT,
            ],
        )?;
        let laser_args = DynamicUniform::new(
            factory,
            pso::ShaderStageFlags::VERTEX | pso::ShaderStageFlags::FRAGMENT,
        )?;
        let note_args = DynamicUniform::new(
            factory,
            pso::ShaderStageFlags::VERTEX | pso::ShaderStageFlags::FRAGMENT,
        )?;
        let pipeline_layout = unsafe {
            factory.device().create_pipeline_layout(
                [env.raw_layout(), laser_args.raw_layout()].iter().cloned(),
                None as Option<(_, _)>,
            )
        }?;

        let vertex_desc = vec![
            (PosTex::vertex(), pso::VertexInputRate::Vertex),
            (VertexArgs::vertex(), pso::VertexInputRate::Instance(1)),
        ];

        let mut shaders = LASER_SHADERS.build(factory, Default::default())?;

        let stencil_face = pso::StencilFace {
            fun: pso::Comparison::Equal,
            op_fail: pso::StencilOp::Replace,
            op_depth_fail: pso::StencilOp::Keep,
            op_pass: pso::StencilOp::Keep,
        };

        let pipe_desc = PipelineDescBuilder::new()
            .with_vertex_desc(&vertex_desc)
            .with_shaders(shaders.raw()?)
            .with_layout(&pipeline_layout)
            .with_subpass(subpass)
            .with_framebuffer_size(framebuffer_width, framebuffer_height)
            .with_depth_stencil(pso::DepthStencilDesc {
                depth: Some(pso::DepthTest {
                    fun: pso::Comparison::Less,
                    write: false,
                }),
                depth_bounds: false,
                stencil: Some(pso::StencilTest {
                    faces: pso::Sided::new(stencil_face),
                    read_masks: pso::StencilValues::Static(pso::Sided::new(1)),
                    write_masks: pso::StencilValues::Static(pso::Sided::new(1)),
                    reference_values: pso::State::Dynamic,
                }),
            })
            .with_blend_targets(vec![pso::ColorBlendDesc {
                mask: pso::ColorMask::ALL,
                blend: Some(pso::BlendState::ADD),
            }]);

        let mut pipelines = PipelinesBuilder::new()
            .with_pipeline(pipe_desc)
            .build(factory, None)?;

        shaders.dispose(factory);

        let laser_mesh = MeshBuilder::new()
            .with_vertices(
                [
                    ([0., 0., 0.], [0., 0.]),
                    ([1., 0., 0.], [1., 0.]),
                    ([1., 0., 1.], [1., 0.]),
                    ([0., 0., 1.], [0., 0.]),
                ]
                .iter()
                .cloned()
                .map(|(p, t)| PosTex {
                    position: p.into(),
                    tex_coord: t.into(),
                })
                .collect::<Vec<_>>(),
            )
            .with_indices(&[0u32, 1, 2, 0, 2, 3][..])
            .build(queue, factory)?;

        Ok(Box::new(DrawLaser::<B> {
            pipeline: pipelines.pop().unwrap(),
            pipeline_layout,
            env,
            laser_args,
            note_args,
            lasers: DynamicVertexBuffer::new(),
            notes: DynamicVertexBuffer::new(),
            instances: Vec::new(),
            square_mesh: laser_mesh,
        }))
    }
}

fn basis_to_points(arr: &Vec<Vector3<f32>>) -> Matrix4<f32> {
    let m = Matrix4::from_iterator(
        (0..3)
            .flat_map(|i| (0..4).map(move |j| arr[j][i]))
            .chain(iter::repeat(1.).take(4)),
    )
    .transpose();
    let v = m.try_inverse().unwrap() * arr[4].push(1.);
    m * (Matrix4::from_iterator(
        (0..4).flat_map(|i| (0..4).map(move |j| if i == j { v[i] } else { 0. })),
    )
    .transpose())
}

#[derive(Debug)]
pub struct DrawLaser<B: Backend> {
    pipeline: B::GraphicsPipeline,
    pipeline_layout: B::PipelineLayout,
    env: EnvironmentSub<B>,
    laser_args: DynamicUniform<B, LaserArgs>,
    note_args: DynamicUniform<B, LaserArgs>,
    lasers: DynamicVertexBuffer<B, VertexArgs>,
    notes: DynamicVertexBuffer<B, VertexArgs>,
    instances: Vec<u32>,
    square_mesh: Mesh<B>,
}

impl<B: Backend> RenderGroup<B, World> for DrawLaser<B> {
    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _: QueueId,
        index: usize,
        _: Subpass<B>,
        world: &World,
    ) -> PrepareResult {
        let (entities, options, lasers, notes, transforms, hierarchy) = <(
            Entities,
            ReadExpect<LaserOptions>,
            ReadStorage<Laser>,
            ReadStorage<Note>,
            ReadStorage<Transform>,
            ReadExpect<ParentHierarchy>,
        )>::fetch(world);
        self.env.process(factory, index, world);

        let basis: [f32; 3] = options.basis.coords.into();
        let start_z = 0.;
        let end_z = 1.;
        let cutoff = 0.7;
        let note_len = 0.02;

        let source: Vec<_> = [
            [0., 0., start_z],
            [1., 0., start_z],
            [0., 1., start_z],
            [0., 0., end_z],
            [1., 1., end_z],
        ]
        .iter()
        .map(|x| Vector3::from_column_slice(x))
        .collect();

        let split_inner = |x: Point3<f32>| options.basis.coords * cutoff + x.coords * (1. - cutoff);

        let target: Vec<_> = [
            options.judge_quad[0].coords,
            options.judge_quad[1].coords,
            options.judge_quad[3].coords,
            split_inner(options.judge_quad[0]),
            split_inner(options.judge_quad[2]),
        ]
        .to_vec();

        let identity: [[f32; 4]; 4] = Matrix4::identity().into();
        let transform: [[f32; 4]; 4] =
            (basis_to_points(&target) * basis_to_points(&source).try_inverse().unwrap()).into();

        let note_transform: [[f32; 4]; 4] = Matrix4::new_translation(&Vector3::new(0., 0., -0.5))
            .append_nonuniform_scaling(&Vector3::new(1., 1., note_len))
            .into();

        let laser_args = LaserArgs {
            basis: basis.into(),
            pre_transform: identity.into(),
            post_transform: transform.into(),
        };

        let note_args = LaserArgs {
            basis: basis.into(),
            pre_transform: note_transform.into(),
            post_transform: transform.into(),
        };
        self.laser_args.write(factory, index, laser_args.std140());
        self.note_args.write(factory, index, note_args.std140());
        let laser_vertex_args: Vec<_> = (&lasers, &transforms)
            .join()
            .map(|(l, t)| {
                let (r, g, b) = l.color.into_components();
                VertexArgs {
                    tint: [r, g, b, 1.].into(),
                    ..VertexArgs::from_object_data(t, None)
                }
            })
            .collect();
        self.instances.clear();
        self.instances.push(0);
        let mut note_vertex_args = Vec::new();
        for (e, _) in (&entities, &lasers).join() {
            note_vertex_args.extend((&notes, &transforms, hierarchy.all_children(e)).join().map(
                |(_, t, _)| VertexArgs {
                    tint: [0., 0., 0., 1.].into(),
                    ..VertexArgs::from_object_data(t, None)
                },
            ));
            self.instances.push(note_vertex_args.len() as u32);
        }
        self.lasers.write(
            factory,
            index,
            laser_vertex_args.len() as u64,
            &[laser_vertex_args],
        );
        self.notes.write(
            factory,
            index,
            note_vertex_args.len() as u64,
            &[note_vertex_args],
        );
        PrepareResult::DrawRecord
    }

    fn draw_inline(
        &mut self,
        mut encoder: RenderPassEncoder<B>,
        index: usize,
        _: Subpass<B>,
        _: &World,
    ) {
        encoder.bind_graphics_pipeline(&self.pipeline);
        self.env.bind(index, &self.pipeline_layout, 0, &mut encoder);
        for (i, window) in self.instances.windows(2).enumerate() {
            unsafe {
                encoder.set_stencil_reference(pso::Face::FRONT | pso::Face::BACK, 0);
            }
            self.note_args
                .bind(index, &self.pipeline_layout, 1, &mut encoder);
            self.notes.bind(index, 1, 0, &mut encoder);
            self.square_mesh
                .bind_and_draw(0, &[PosTex::vertex()], window[0]..window[1], &mut encoder)
                .unwrap();
            unsafe {
                encoder.set_stencil_reference(pso::Face::FRONT | pso::Face::BACK, 1);
            }
            self.laser_args
                .bind(index, &self.pipeline_layout, 1, &mut encoder);
            self.lasers.bind(index, 1, 0, &mut encoder);
            self.square_mesh
                .bind_and_draw(0, &[PosTex::vertex()], i as u32..i as u32 + 1, &mut encoder)
                .unwrap();
        }
    }

    fn dispose(self: Box<Self>, factory: &mut Factory<B>, _: &World) {
        unsafe {
            factory.device().destroy_graphics_pipeline(self.pipeline);
            factory
                .device()
                .destroy_pipeline_layout(self.pipeline_layout);
        }
    }
}

#[derive(Debug)]
pub struct RenderLaser;

impl<B: Backend> RenderPlugin<B> for RenderLaser {
    fn on_plan(
        &mut self,
        plan: &mut RenderPlan<B>,
        _: &mut Factory<B>,
        _: &World,
    ) -> Result<(), amethyst::Error> {
        plan.extend_target(Target::default(), move |ctx| {
            ctx.add(
                RenderOrder::AfterTransparent,
                DrawLaserDesc::<B>::new().builder(),
            )
        });
        Ok(())
    }
}
