use crate::chart::PlaySettings;
use crate::{laser, InterFont};
use amethyst::{
    animation::{
        Animation, AnimationCommand, AnimationControl, AnimationControlSet, ControlState,
        EndControl, InterpolationFunction, Sampler, SamplerControlSet, SamplerPrimitive,
        UiTransformChannel,
    },
    assets::{AssetStorage, Handle},
    core::{
        math::{Matrix3, Point3, Vector2},
        timing::Time,
        transform::Transform,
        Parent, SystemDesc,
    },
    ecs::{Entities, Join, Read, ReadExpect, ReadStorage, System, SystemData, World, WriteStorage},
    shrev::{EventChannel, ReaderId},
    ui::{Anchor, ScaleMode, UiText, UiTransform},
    winit::{ElementState, Event, KeyboardInput, ScanCode, WindowEvent},
};
use serde::{Deserialize, Serialize};

const PERFECT_WINDOW: f32 = 0.04;
const NEAR_WINDOW: f32 = 0.08;
const EARLY_MISS_WINDOW: f32 = 0.15;

pub struct JudgeSystem {
    reader_id: ReaderId<Event>,
    mapping: Vec<(ScanCode, (f32, f32))>,
    animation: Handle<Animation<UiTransform>>,
}

pub struct JudgeSystemDesc {
    pub mapping: ScancodeMap,
}

impl<'a, 'b> SystemDesc<'a, 'b, JudgeSystem> for JudgeSystemDesc {
    fn build(self, world: &mut World) -> JudgeSystem {
        <JudgeSystem as System<'_>>::SystemData::setup(world);

        let reader_id = world
            .get_mut::<EventChannel<Event>>()
            .unwrap()
            .register_reader();

        world.insert(AssetStorage::<Sampler<SamplerPrimitive<f32>>>::default());
        use SamplerPrimitive::Vec2;
        let sampler = world
            .get_mut::<AssetStorage<Sampler<SamplerPrimitive<f32>>>>()
            .unwrap()
            .insert(Sampler {
                input: vec![0., 0.3],
                output: vec![Vec2([0., 0.]), Vec2([0., 0.1])],
                function: InterpolationFunction::SphericalLinear,
            });

        world.insert(AssetStorage::<Animation<UiTransform>>::default());
        let animation = world
            .get_mut::<AssetStorage<Animation<UiTransform>>>()
            .unwrap()
            .insert(Animation {
                nodes: vec![(0, UiTransformChannel::Translation, sampler)],
            });

        JudgeSystem {
            reader_id,
            mapping: self.mapping.into_mapping(),
            animation,
        }
    }
}

impl<'s> System<'s> for JudgeSystem {
    type SystemData = (
        Entities<'s>,
        Read<'s, EventChannel<Event>>,
        ReadExpect<'s, Time>,
        Read<'s, Option<PlaySettings>>,
        WriteStorage<'s, laser::Note>,
        ReadStorage<'s, Transform>,
        ReadExpect<'s, InterFont>,
        WriteStorage<'s, UiText>,
        WriteStorage<'s, UiTransform>,
        WriteStorage<'s, AnimationControlSet<(), UiTransform>>,
        WriteStorage<'s, SamplerControlSet<UiTransform>>,
        WriteStorage<'s, Parent>,
    );

    fn run(
        &mut self,
        (
            entities,
            events,
            time,
            settings,
            mut notes,
            transforms,
            inter_font,
            mut ui_text,
            mut ui_transform,
            mut anim,
            mut samp,
            mut parent,
        ): Self::SystemData,
    ) {
        for event in events.read(&mut self.reader_id) {
            match event {
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            input:
                                KeyboardInput {
                                    scancode,
                                    state: ElementState::Pressed,
                                    ..
                                },
                            ..
                        },
                    ..
                } => {
                    if let Some(PlaySettings {
                        base_time,
                        offset,
                        norm_threshold,
                        ..
                    }) = *settings
                    {
                        let rel = (time.absolute_time_seconds() - base_time) as f32 + offset;
                        if let Ok(input_pos_idx) =
                            self.mapping.binary_search_by_key(&scancode, |(s, _)| s)
                        {
                            let input_pos = {
                                let (_, (x, y)) = self.mapping[input_pos_idx];
                                Vector2::new(x, y)
                            };
                            if let Some((entity, diff, pos, _)) =
                                (&entities, &mut notes, &transforms)
                                    .join()
                                    .map(|(e, n, t)| (e, n.time - rel, t))
                                    .filter(|(_, diff, _)| {
                                        (-NEAR_WINDOW..EARLY_MISS_WINDOW).contains(&diff)
                                    })
                                    .map(|(e, n, t)| {
                                        let note_pos = t
                                            .global_matrix()
                                            .transform_point(&Point3::new(0.5, 0., 0.))
                                            .xy();
                                        let norm = Matrix3::new_nonuniform_scaling(&Vector2::new(
                                            1.0, 0.3,
                                        ))
                                        .transform_vector(&(input_pos - note_pos.coords))
                                        .norm_squared();
                                        (e, n, note_pos, norm)
                                    })
                                    .filter(|(_, _, _, norm)| norm <= &norm_threshold)
                                    .min_by(|(_, lhs_time, _, lhs), (_, rhs_time, _, rhs)| {
                                        // TODO: relying on equality is not good
                                        lhs.partial_cmp(rhs)
                                            .unwrap()
                                            .then_with(|| lhs_time.partial_cmp(&rhs_time).unwrap())
                                    })
                            {
                                let (text, color) =
                                    if (-PERFECT_WINDOW..PERFECT_WINDOW).contains(&diff) {
                                        ("PERFECT", [0.8, 0., 0.8, 1.])
                                    } else if (-NEAR_WINDOW..NEAR_WINDOW).contains(&diff) {
                                        ("NEAR", [0., 0.1, 0.8, 1.])
                                    } else {
                                        ("MISS", [0.9, 0., 0.2, 1.])
                                    };
                                let ui_entity = entities.create();
                                let ui_entity_parent = entities.create();
                                parent
                                    .insert(ui_entity, Parent::new(ui_entity_parent))
                                    .unwrap();
                                let text =
                                    UiText::new(inter_font.0.clone(), text.into(), color, 40.);
                                let mut ui_trans_parent = UiTransform::new(
                                    String::from("JudgeParent"),
                                    Anchor::BottomLeft,
                                    Anchor::BottomMiddle,
                                    pos.x,
                                    pos.y,
                                    0.,
                                    0.3,
                                    1.,
                                );
                                ui_trans_parent.scale_mode = ScaleMode::Percent;
                                let mut ui_trans = UiTransform::new(
                                    String::from("Judge"),
                                    Anchor::BottomMiddle,
                                    Anchor::BottomMiddle,
                                    0.,
                                    0.,
                                    0.,
                                    0.3,
                                    0.1,
                                );
                                ui_trans.scale_mode = ScaleMode::Percent;
                                ui_text.insert(ui_entity, text).unwrap();
                                ui_transform
                                    .insert(ui_entity_parent, ui_trans_parent)
                                    .unwrap();
                                ui_transform.insert(ui_entity, ui_trans).unwrap();
                                let mut control_set = AnimationControlSet::default();
                                control_set.insert(
                                    (),
                                    AnimationControl::new(
                                        self.animation.clone(),
                                        EndControl::Stay,
                                        ControlState::Requested,
                                        AnimationCommand::Start,
                                        1.0,
                                    ),
                                );
                                anim.insert(ui_entity, control_set).unwrap();
                                entities.delete(entity).unwrap();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (eid, parent, samp) in (&entities, &parent, &mut samp).join() {
            if let Some(ControlState::Done) = &samp.samplers.get(0).map(|x| &x.state) {
                entities.delete(parent.entity).unwrap();
            }
        }
        if let Some(PlaySettings {
            base_time, offset, ..
        }) = *settings
        {
            let rel = (time.absolute_time_seconds() - base_time) as f32 + offset;
            for (entity, _, t) in (&entities, &mut notes, &transforms)
                .join()
                .filter(|(_, n, _)| n.time + NEAR_WINDOW < rel)
            {
                let pos = t
                    .global_matrix()
                    .transform_point(&Point3::new(0.5, 0., 0.))
                    .xy();
                let (text, color) = ("MISS", [0.9, 0., 0.2, 1.]);
                let ui_entity = entities.create();
                let ui_entity_parent = entities.create();
                parent
                    .insert(ui_entity, Parent::new(ui_entity_parent))
                    .unwrap();
                let text = UiText::new(inter_font.0.clone(), text.into(), color, 40.);
                let mut ui_trans_parent = UiTransform::new(
                    String::from("JudgeParent"),
                    Anchor::BottomLeft,
                    Anchor::BottomMiddle,
                    pos.x,
                    pos.y,
                    0.,
                    0.3,
                    1.,
                );
                ui_trans_parent.scale_mode = ScaleMode::Percent;
                let mut ui_trans = UiTransform::new(
                    String::from("Judge"),
                    Anchor::BottomMiddle,
                    Anchor::BottomMiddle,
                    0.,
                    0.,
                    0.,
                    0.3,
                    0.1,
                );
                ui_trans.scale_mode = ScaleMode::Percent;
                ui_text.insert(ui_entity, text).unwrap();
                ui_transform
                    .insert(ui_entity_parent, ui_trans_parent)
                    .unwrap();
                ui_transform.insert(ui_entity, ui_trans).unwrap();
                let mut control_set = AnimationControlSet::default();
                control_set.insert(
                    (),
                    AnimationControl::new(
                        self.animation.clone(),
                        EndControl::Stay,
                        ControlState::Requested,
                        AnimationCommand::Start,
                        1.0,
                    ),
                );
                anim.insert(ui_entity, control_set).unwrap();
                entities.delete(entity).unwrap();
            }
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct ScancodeRow {
    offset: f32,
    keys: Vec<ScanCode>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct ScancodeMap {
    width: f32,
    rows: Vec<ScancodeRow>,
}

impl ScancodeMap {
    fn into_mapping(self) -> Vec<(ScanCode, (f32, f32))> {
        let height = self.rows.len() as f32;
        let mut ret: Vec<_> = self
            .rows
            .into_iter()
            .enumerate()
            .flat_map(|(i, r)| {
                let width = r.keys.len() as f32;
                let offset = r.offset;
                r.keys
                    .into_iter()
                    .enumerate()
                    .map(move |(j, k)| (k, (i as f32 / height, (offset + j as f32) / width)))
            })
            .collect();
        ret.sort_unstable_by(|x, y| x.partial_cmp(y).unwrap());
        ret
    }
}
