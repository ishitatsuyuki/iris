extern crate amethyst;

use amethyst::{
    assets::Processor,
    core::{
        math::{Matrix4, Point3, Quaternion, Translation3, Vector3},
        timing,
        transform::{Transform, TransformBundle},
    },
    ecs::{Entity, Join, ReadExpect, ReadStorage, System, WriteStorage},
    prelude::*,
    renderer::{
        camera::Projection,
        plugins::{RenderFlat3D, RenderToWindow},
        types::DefaultBackend,
        Camera, RenderingBundle,
    },
    utils::application_root_dir,
};

mod laser;
use laser::{Laser, LaserOptions, RenderLaser};

pub struct MoveLaserSystem;
impl<'s> System<'s> for MoveLaserSystem {
    type SystemData = (
        ReadExpect<'s, timing::Time>,
        ReadStorage<'s, Laser>,
        WriteStorage<'s, Transform>,
    );

    fn run(&mut self, (time, lasers, mut transforms): Self::SystemData) {
        return;
        for (_, transform) in (&lasers, &mut transforms).join() {
            transform.set_translation_y(f64::sin(time.absolute_time_seconds()).abs() as f32);
        }
    }
}

struct MainStage;

impl MainStage {
    fn initialize_camera(&mut self, world: &mut World, proj: Projection) {
        let perspective_inv = proj.as_matrix().try_inverse().unwrap();
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
        world.insert(LaserOptions { judge_quad, basis });
        world
            .create_entity()
            .with(Camera::from(proj))
            .with(Transform::default())
            .build();
    }
}

impl SimpleState for MainStage {
    fn on_start(&mut self, data: StateData<'_, GameData<'_, '_>>) {
        let proj = Projection::perspective(4.0 / 3.0, 90.0, 0.01, 100.0);
        self.initialize_camera(data.world, proj);
        data.world.register::<Laser>();
        data.world
            .create_entity()
            .with(Laser {
                color: (0.8, 0.1, 0.1).into(),
            })
            .with(Transform::default())
            .build();
        let mut transform = Transform::default();
        transform.set_translation_y(0.2);
        data.world
            .create_entity()
            .with(Laser {
                color: (0., 0.1, 0.8).into(),
            })
            .with(transform)
            .build();
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
            // which will always execute on the main thread.
            RenderingBundle::<DefaultBackend>::new()
                // The RenderToWindow plugin provides all the scaffolding for opening a window and drawing on it
                .with_plugin(
                    RenderToWindow::from_config_path(display_config).with_clear([0., 0., 0., 1.]),
                )
                .with_plugin(RenderFlat3D::default())
                .with_plugin(RenderLaser),
        )?
        .with(MoveLaserSystem, "move_laser", &[]);

    let mut game = Application::new(resources, MainStage, game_data)?;
    game.run();

    Ok(())
}
