mod usc;

use crate::plugins::camera::Camera;
use crate::transform::Transform;
use bevy_app::prelude::*;
use bevy_diagnostic::{
    DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
};
use bevy_easings::{custom_ease_system, EasingChainComponent, EasingComponent};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use bevy_time::common_conditions::on_real_timer;
use glam::{Quat, Vec3};
use itertools::Itertools;
use std::fs::{create_dir, File};
use std::io::{BufWriter, ErrorKind, Write};
use std::time::Duration;

pub struct BenchmarkPlugin;

impl Plugin for BenchmarkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Benchmark(Vec::new()))
            .insert_state(BenchmarkState::NotRunning)
            .add_systems(OnEnter(BenchmarkState::Running), prepare_benchmark)
            .add_systems(
                Update,
                (
                    measure,
                    check_if_finished.after(custom_ease_system::<Transform>),
                )
                    .chain()
                    .run_if(in_state(BenchmarkState::Running))
                    .run_if(on_real_timer(Duration::from_millis(1000))),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
enum BenchmarkState {
    NotRunning,
    Running,
}

#[derive(Debug, Default, Resource)]
struct Benchmark(Vec<BenchmarkData>);

#[derive(Debug)]
struct BenchmarkData {
    fps: f64,
    cpu: f64,
}

fn save(benchmark: &Benchmark) -> std::io::Result<()> {
    let dir = std::env::current_dir()?.join("benchmarks");
    if let Err(err) = create_dir(&dir) {
        match err.kind() {
            ErrorKind::AlreadyExists => {}
            _ => return Err(err),
        }
    }

    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let file_path = dir.join(time.as_secs().to_string()).with_extension("txt");

    let file = File::create(file_path)?;
    let mut buf = BufWriter::new(file);

    let fps = benchmark
        .0
        .iter()
        .map(|data| format!("{:.0}", data.fps))
        .join(",");

    buf.write_all(fps.as_bytes())?;

    buf.write_all(b"\n")?;

    let cpu = benchmark
        .0
        .iter()
        .map(|data| format!("{:.0}", data.cpu))
        .join(",");

    buf.write_all(cpu.as_bytes())?;

    Ok(())
}

fn prepare_benchmark(
    mut benchmark: ResMut<Benchmark>,
    mut commands: Commands,
    query: Query<(Entity, &Transform), With<Camera>>,
) {
    benchmark.0 = Vec::with_capacity(60);

    let (entity, transform) = query.get_single().unwrap();
    use bevy_easings::*;

    let start = transform
        .ease_to(
            *transform,
            EaseMethod::Discrete,
            EasingType::Once {
                duration: Duration::ZERO,
            },
        )
        .ease_to(
            *transform,
            EaseMethod::Discrete,
            EasingType::Once {
                duration: Duration::ZERO,
            },
        );

    let ease = usc::TRANSFORMS.iter().fold(start, |acc, transform| {
        acc.ease_to(
            *transform,
            EaseMethod::Linear,
            EasingType::Once {
                duration: Duration::from_millis(2000),
            },
        )
    });

    commands.entity(entity).insert(ease);
}

fn measure(diagnostics: Res<DiagnosticsStore>, mut benchmark: ResMut<Benchmark>) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .unwrap_or(0.0);

    let cpu = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::CPU_USAGE)
        .and_then(|cpu| cpu.smoothed())
        .unwrap_or(0.0);

    benchmark.0.push(BenchmarkData { fps, cpu });
}

fn check_if_finished(
    query: Query<
        Entity,
        (
            With<Camera>,
            Or<(
                With<EasingComponent<Transform>>,
                With<EasingChainComponent<Transform>>,
            )>,
        ),
    >,
    mut next_state: ResMut<NextState<BenchmarkState>>,
) {
    if query.is_empty() {
        next_state.set(BenchmarkState::NotRunning);
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    if ui.button("Print Transform").clicked() {
        let mut q = world.query_filtered::<&Transform, With<Camera>>();
        log::error!("{:?}", q.get_single(world).unwrap());
    }

    let benchmark_state = *world.get_resource::<State<BenchmarkState>>().unwrap().get();

    match benchmark_state {
        BenchmarkState::NotRunning => {
            if ui.button("Start Benchmark").clicked() {
                world
                    .get_resource_mut::<NextState<BenchmarkState>>()
                    .unwrap()
                    .set(BenchmarkState::Running);
            }

            if ui.button("Write to file").clicked() {
                world.run_system_once(|benchmark: Res<Benchmark>| match save(&benchmark) {
                    Ok(_) => {
                        log::info!("Wrote file");
                    }
                    Err(err) => {
                        log::error!("{}", err);
                    }
                });
            }
        }
        BenchmarkState::Running => {
            if ui.button("Stop Benchmark").clicked() {
                world
                    .get_resource_mut::<NextState<BenchmarkState>>()
                    .unwrap()
                    .set(BenchmarkState::NotRunning);
            }
        }
    }
}
