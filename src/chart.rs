use crate::laser;
use amethyst::{
    core::{
        math::Vector3,
        timing::Time,
        transform::{Parent, Transform},
    },
    ecs::{Entities, Entity, Read, ReadExpect, System, Write, WriteStorage},
    renderer::palette::rgb::LinSrgb,
};
use std::collections::BTreeMap;
use std::ops::{Deref, Range};
use superslice::Ext;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub struct LaserId(pub u32);

#[derive(Debug)]
pub struct Note {
    pub laser: LaserId,
    pub lane: u32,
}

#[derive(Debug)]
pub struct BpmCommand {
    pub bpm: f32,
    pub position: f32,
}

#[derive(Debug)]
pub enum LaserCommand {
    Enter {
        y: f32,
        lanes: u16,
        color: LinSrgb<f32>,
    },
    Leave,
    LineTo {
        time: Timed<()>,
        y: f32,
    },
}

#[derive(Debug)]
pub struct Chart {
    /// All notes sorted by time.
    pub notes: Vec<Timed<Note>>,
    /// BPM change sequences sorted by time.
    pub bpm: Vec<Timed<BpmCommand>>,
    /// Laser sequences sorted by time.
    pub lasers: Vec<Timed<(LaserId, LaserCommand)>>,
    pub default_bpm: f32,
}

pub struct NoteSystem;

pub struct PlaySettings {
    /// The margin between note appearance and judgement in seconds.
    pub speed: f32,
    pub base_time: f64,
}
pub struct ChartState {
    /// The window of transforms z where we draw.
    pub draw_window: Range<f32>,
    /// Relative position to cut off the laser origin.
    pub cutoff: f32,
    lasers: BTreeMap<LaserId, Entity>,
    /// The time up to which we have loaded.
    last_time: f32,
}
impl Default for ChartState {
    fn default() -> Self {
        Self {
            draw_window: 0. ..0.,
            cutoff: 0.7,
            lasers: BTreeMap::new(),
            last_time: 0.,
        }
    }
}

#[derive(Debug)]
pub struct Timed<T> {
    pub time: f32,
    pub inner: T,
}
impl<T> Deref for Timed<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

fn equal_range_by_time<T>(slice: &[Timed<T>], lo: f32, hi: f32) -> Range<usize> {
    use std::cmp::Ordering::*;
    assert!(lo <= hi);
    slice.equal_range_by(|x| {
        if x.time < lo {
            Less
        } else if x.time >= hi {
            Greater
        } else {
            Equal
        }
    })
}

fn position_for_time(bpms: &[Timed<BpmCommand>], time: f32) -> f32 {
    let lower_bound = &bpms[bpms
        .lower_bound_by(|x| x.time.partial_cmp(&time).unwrap())
        .saturating_sub(1)];
    lower_bound.position + (time - lower_bound.time) * lower_bound.bpm / 60.0
}

impl<'s> System<'s> for NoteSystem {
    type SystemData = (
        Entities<'s>,
        ReadExpect<'s, Time>,
        Read<'s, Option<Chart>>,
        Read<'s, Option<PlaySettings>>,
        Write<'s, ChartState>,
        WriteStorage<'s, Parent>,
        WriteStorage<'s, laser::Laser>,
        WriteStorage<'s, laser::Note>,
        WriteStorage<'s, Transform>,
    );

    fn run(
        &mut self,
        (
            entities,
            time,
            chart,
            settings,
            mut state,
            mut parents,
            mut laser_storage,
            mut note_storage,
            mut transforms,
        ): Self::SystemData,
    ) {
        if let Some(settings) = &*settings {
            let chart = chart.as_ref().unwrap();
            let notes = &chart.notes;
            let lasers = &chart.lasers;

            let now_rel = (time.absolute_time_seconds() - settings.base_time) as f32;
            let start_pos = position_for_time(&chart.bpm, now_rel);
            let end_pos = position_for_time(&chart.bpm, now_rel + settings.speed);
            let cutoff = 0.7 * (end_pos - start_pos) / (chart.default_bpm / 60.) / settings.speed;
            let clamped_cutoff = cutoff.min(0.95);
            let clamped_end_pos = start_pos + (end_pos - start_pos) * clamped_cutoff / cutoff;

            for to_load in &lasers[equal_range_by_time(lasers, state.last_time, now_rel)] {
                match to_load.1 {
                    LaserCommand::Enter { y, lanes, color } => {
                        let eid = entities.create();
                        laser_storage
                            .insert(eid, laser::Laser { color, lanes })
                            .unwrap();
                        let mut transform = Transform::default();
                        transform.set_translation_y(y);
                        transforms.insert(eid, transform).unwrap();
                        assert!(state.lasers.insert(to_load.0, eid).is_none());
                    }
                    LaserCommand::Leave => {
                        let eid = state.lasers.remove(&to_load.0).unwrap();
                        entities.delete(eid).unwrap();
                    }
                    LaserCommand::LineTo { .. } => unimplemented!(),
                }
            }
            for to_load in &notes[equal_range_by_time(
                notes,
                state.last_time + settings.speed,
                now_rel + settings.speed,
            )] {
                let eid = entities.create();
                note_storage.insert(eid, laser::Note {}).unwrap();
                let laser_id = state.lasers[&to_load.laser];
                let laser = laser_storage.get(laser_id).unwrap();
                parents.insert(eid, Parent::new(laser_id)).unwrap();

                let mut transform = Transform::default();
                transform.set_translation_x(to_load.lane as f32 / laser.lanes as f32);
                transform.set_scale(Vector3::new(1. / laser.lanes as f32, 1., 1.));
                transform.set_translation_z(position_for_time(&chart.bpm, to_load.time));
                transforms.insert(eid, transform).unwrap();
            }

            state.cutoff = clamped_cutoff;
            state.draw_window = start_pos..clamped_end_pos;
            state.last_time = now_rel;
        }
    }
}
