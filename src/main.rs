extern crate amethyst;

use amethyst::{
    core::{
        math::{Matrix4, Point3},
        timing::Time,
        transform::{Transform, TransformBundle},
    },
    ecs::{Join, ReadStorage, System, Write},
    prelude::*,
    renderer::{
        camera::Projection,
        plugins::{RenderFlat3D, RenderToWindow},
        types::DefaultBackend,
        Camera, RenderingBundle,
    },
    utils::{application_root_dir, auto_fov::AutoFovSystem},
};

mod laser;
use crate::chart::{BpmCommand, Chart, LaserCommand, LaserId, Note, PlaySettings, Timed};
use chart::NoteSystem;
use laser::{LaserOptions, RenderLaser};

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
                    RenderToWindow::from_config_path(display_config).with_clear([0., 0., 0., 1.]),
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
