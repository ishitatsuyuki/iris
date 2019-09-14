extern crate amethyst;

use amethyst::{
    core::{
        math::{Matrix4, Point3},
        timing::Time,
        transform::{Transform, TransformBundle},
        SystemBundle,
    },
    ecs::{DispatcherBuilder, Join, ReadExpect, ReadStorage, System, SystemData, Write},
    prelude::*,
    renderer::{
        bundle::{ImageOptions, OutputColor, RenderPlan, RenderPlugin, Target, TargetPlanOutputs},
        camera::Projection,
        plugins::RenderFlat3D,
        rendy::hal::{
            command::{ClearColor, ClearDepthStencil, ClearValue},
            format::{Format, ImageFeature},
            PhysicalDevice,
        },
        types::DefaultBackend,
        Backend, Camera, Factory, Kind, RenderingBundle,
    },
    utils::{application_root_dir, auto_fov::AutoFovSystem},
    window::{DisplayConfig, ScreenDimensions, Window, WindowBundle},
};

mod laser;
use crate::chart::{BpmCommand, Chart, LaserCommand, LaserId, Note, PlaySettings, Timed};
use chart::NoteSystem;
use laser::{LaserOptions, RenderLaser};
use std::path::Path;

mod chart;

pub struct LaserFovSystem {
    last_matrix: Matrix4<f32>,
}
impl LaserFovSystem {
    fn new() -> Self {
        Self {
            last_matrix: Matrix4::identity(),
        }
    }
}
impl<'s> System<'s> for LaserFovSystem {
    type SystemData = (ReadStorage<'s, Camera>, Write<'s, LaserOptions>);

    fn run(&mut self, (cameras, mut options): Self::SystemData) {
        let proj = cameras.join().next().unwrap().as_matrix();
        if proj != &self.last_matrix {
            let perspective_inv = proj.try_inverse().unwrap();
            let reverse_point = |x, y, target_z| {
                let near = perspective_inv.transform_point(&Point3::new(x, y, 0.));
                let near_far = perspective_inv.transform_point(&Point3::new(x, y, 1.)) - near;
                let unit = near_far / near_far.z;
                near + (target_z - near.z) * unit
            };
            let judge_quad: Vec<_> = [(-1., 1.), (1., 1.), (1., -1.), (-1., -1.)]
                .iter()
                .map(|&(x, y)| reverse_point(x, y, -1.))
                .collect();
            let basis = reverse_point(0., -1., -5.);
            *options = LaserOptions { judge_quad, basis };
            self.last_matrix = proj.clone();
        }
    }
}

struct MainStage;

impl MainStage {
    fn initialize_camera(&mut self, world: &mut World, proj: Projection) {
        world
            .create_entity()
            .with(Camera::from(proj))
            .with(Transform::default())
            .build();
    }

    fn initialize_chart(&mut self, world: &mut World) {
        world.register::<laser::Note>();
        world.register::<laser::Laser>();
        let now = world.fetch::<Time>().absolute_time_seconds();
        world.insert(Some(PlaySettings {
            speed: 1.0,
            base_time: now,
        }));
        world.insert(Some(Chart {
            notes: (0..32)
                .flat_map(|i| {
                    vec![
                        Timed {
                            time: 0.075 * (2 * i) as f32 + 1.0,
                            inner: Note {
                                laser: LaserId(0),
                                lane: 1,
                            },
                        },
                        Timed {
                            time: 0.075 * (2 * i + 1) as f32 + 1.0,
                            inner: Note {
                                laser: LaserId(0),
                                lane: 2,
                            },
                        },
                    ]
                })
                .collect(),
            bpm: vec![Timed {
                time: 0.0,
                inner: BpmCommand {
                    bpm: 200.,
                    position: 0.0,
                },
            }],
            lasers: vec![Timed {
                time: 0.0,
                inner: (
                    LaserId(0),
                    LaserCommand::Enter {
                        y: 0.3,
                        lanes: 4,
                        color: (0., 0.1, 0.8).into(),
                    },
                ),
            }],
            default_bpm: 200.0,
        }))
    }
}

impl SimpleState for MainStage {
    fn on_start(&mut self, data: StateData<'_, GameData<'_, '_>>) {
        let proj = Projection::perspective(4.0 / 3.0, 90.0, 0.01, 100.0);
        self.initialize_camera(data.world, proj);
        self.initialize_chart(data.world);
    }
}

#[derive(Default, Debug)]
struct RenderToWindowWithStencil {
    dirty: bool,
    clear: Option<ClearColor>,
    depth_clear: Option<ClearDepthStencil>,
    config: Option<DisplayConfig>,
    dimensions: Option<ScreenDimensions>,
}

impl RenderToWindowWithStencil {
    /// Create RenderToWindow plugin with [`WindowBundle`] using specified config path.
    pub fn from_config_path(path: impl AsRef<Path>) -> Self {
        Self::from_config(DisplayConfig::load(path))
    }

    /// Create RenderToWindow plugin with [`WindowBundle`] using specified config.
    pub fn from_config(display_config: DisplayConfig) -> Self {
        Self {
            config: Some(display_config),
            ..Default::default()
        }
    }

    /// Clear window with specified color every frame.
    pub fn with_clear(mut self, clear: impl Into<ClearColor>) -> Self {
        self.clear = Some(clear.into());
        self
    }
}

impl<B: Backend> RenderPlugin<B> for RenderToWindowWithStencil {
    fn on_build<'a, 'b>(
        &mut self,
        world: &mut World,
        builder: &mut DispatcherBuilder<'a, 'b>,
    ) -> Result<(), amethyst::error::Error> {
        if let Some(config) = self.config.take() {
            WindowBundle::from_config(config).build(world, builder)?;
        }

        Ok(())
    }

    #[allow(clippy::map_clone)]
    fn should_rebuild(&mut self, world: &World) -> bool {
        let new_dimensions = world.try_fetch::<ScreenDimensions>();
        use std::ops::Deref;
        if self.dimensions.as_ref() != new_dimensions.as_ref().map(|d| d.deref()) {
            self.dirty = true;
            self.dimensions = new_dimensions.map(|d| d.deref().clone());
            return false;
        }
        self.dirty
    }

    fn on_plan(
        &mut self,
        plan: &mut RenderPlan<B>,
        factory: &mut Factory<B>,
        world: &World,
    ) -> Result<(), amethyst::error::Error> {
        self.dirty = false;

        let window = <ReadExpect<'_, Window>>::fetch(world);
        let surface = factory.create_surface(&window);
        let dimensions = self.dimensions.as_ref().unwrap();
        let window_kind = Kind::D2(dimensions.width() as u32, dimensions.height() as u32, 1, 1);

        let format = [
            Format::D24UnormS8Uint,
            Format::D32SfloatS8Uint,
            Format::D16UnormS8Uint,
        ]
        .iter()
        .cloned()
        .filter(|&f| {
            factory
                .physical()
                .format_properties(Some(f))
                .optimal_tiling
                .contains(ImageFeature::DEPTH_STENCIL_ATTACHMENT)
        })
        .next()
        .ok_or_else(|| {
            amethyst::error::Error::from_string("None of the stencil formats are supported")
        })?;

        let depth_options = ImageOptions {
            kind: window_kind,
            levels: 1,
            format,
            clear: Some(ClearValue::DepthStencil(ClearDepthStencil(1.0, 0))),
        };

        plan.add_root(Target::Main);
        plan.define_pass(
            Target::Main,
            TargetPlanOutputs {
                colors: vec![OutputColor::Surface(
                    surface,
                    self.clear.map(ClearValue::Color),
                )],
                depth: Some(depth_options),
            },
        )?;

        Ok(())
    }
}

fn main() -> amethyst::Result<()> {
    amethyst::Logger::from_config(Default::default())
        .level_for("gfx_backend_vulkan", amethyst::LogLevelFilter::Warn)
        .level_for("rendy_factory::factory", amethyst::LogLevelFilter::Warn)
        .level_for(
            "rendy_memory::allocator::dynamic",
            amethyst::LogLevelFilter::Warn,
        )
        .level_for(
            "rendy_graph::node::render::pass",
            amethyst::LogLevelFilter::Warn,
        )
        .level_for("rendy_graph::node::present", amethyst::LogLevelFilter::Warn)
        .level_for("rendy_graph::graph", amethyst::LogLevelFilter::Warn)
        .level_for(
            "rendy_memory::allocator::linear",
            amethyst::LogLevelFilter::Warn,
        )
        .level_for("rendy_wsi", amethyst::LogLevelFilter::Warn)
        .start();

    let app_root = application_root_dir()?;

    let resources = app_root.join("resources");
    let display_config = resources.join("display_config.ron");

    let game_data = GameDataBuilder::default()
        .with_bundle(TransformBundle::new())?
        .with_bundle(
            RenderingBundle::<DefaultBackend>::new()
                // The RenderToWindow plugin provides all the scaffolding for opening a window and drawing on it
                .with_plugin(
                    RenderToWindowWithStencil::from_config_path(display_config)
                        .with_clear([0., 0., 0., 1.]),
                )
                .with_plugin(RenderFlat3D::default())
                .with_plugin(RenderLaser),
        )?
        .with(AutoFovSystem::new(), "auto_fov", &[])
        .with(LaserFovSystem::new(), "laser_fov", &["auto_fov"])
        .with(NoteSystem, "note_system", &[]);

    let mut game = Application::new(resources, MainStage, game_data)?;
    game.run();

    Ok(())
}
