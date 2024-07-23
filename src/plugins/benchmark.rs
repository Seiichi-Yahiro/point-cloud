use std::fs::{create_dir, File};
use std::io::{BufWriter, ErrorKind, Write};
use std::time::Duration;

use bevy_app::prelude::*;
use bevy_diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_easings::{custom_ease_system, EasingChainComponent, EasingComponent};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use bevy_state::prelude::*;
use bevy_time::common_conditions::on_real_timer;
use itertools::Itertools;

use crate::plugins::camera::Camera;
use crate::plugins::cell::{LoadedCells, LoadingCells};
use crate::transform::Transform;

mod usc;

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
    loaded_points: u64,
    loaded_cells: u32,
    should_load: u32,
    loading_cells: u32,
}

fn save(benchmark: &Benchmark) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let mut buf = {
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

        BufWriter::new(file)
    };

    #[cfg(target_arch = "wasm32")]
    let mut buf = Vec::new();

    let fps = benchmark
        .0
        .iter()
        .map(|data| format!("{:.0}", data.fps))
        .join(",");

    buf.write_all(b"fps:(")?;
    buf.write_all(fps.as_bytes())?;
    buf.write_all(b"),\n")?;

    let cpu = benchmark
        .0
        .iter()
        .map(|data| format!("{:.0}", data.cpu))
        .join(",");

    buf.write_all(b"cpu:(")?;
    buf.write_all(cpu.as_bytes())?;
    buf.write_all(b"),\n")?;

    let loaded_points = benchmark
        .0
        .iter()
        .map(|data| format!("{}", data.loaded_points))
        .join(",");

    buf.write_all(b"loaded_points:(")?;
    buf.write_all(loaded_points.as_bytes())?;
    buf.write_all(b"),\n")?;

    let loaded_cells = benchmark
        .0
        .iter()
        .map(|data| format!("{}", data.loaded_cells))
        .join(",");

    buf.write_all(b"loaded_cells:(")?;
    buf.write_all(loaded_cells.as_bytes())?;
    buf.write_all(b"),\n")?;

    let loading_cells = benchmark
        .0
        .iter()
        .map(|data| format!("{}", data.loading_cells))
        .join(",");

    buf.write_all(b"loading_cells:(")?;
    buf.write_all(loading_cells.as_bytes())?;
    buf.write_all(b"),\n")?;

    let should_load = benchmark
        .0
        .iter()
        .map(|data| format!("{}", data.should_load))
        .join(",");

    buf.write_all(b"should_load:(")?;
    buf.write_all(should_load.as_bytes())?;
    buf.write_all(b"),\n")?;

    #[cfg(target_arch = "wasm32")]
    {
        let data = std::str::from_utf8(&buf).unwrap().to_string();
        let req = ehttp::Request::post("https://192.168.178.89:3000/log", buf);
        ehttp::fetch(req, |res| {
            log::debug!("{:?}", res);
        });
        log::info!("{}", data);
    }

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

fn measure(
    diagnostics: Res<DiagnosticsStore>,
    mut benchmark: ResMut<Benchmark>,
    cell_stats: Res<crate::plugins::cell::Stats>,
    loaded_cells: Res<LoadedCells>,
    loading_cells: Res<LoadingCells>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .unwrap_or(0.0);

    /* let cpu = diagnostics
    .get(&SystemInformationDiagnosticsPlugin::CPU_USAGE)
    .and_then(|cpu| cpu.smoothed())
    .unwrap_or(0.0);*/

    benchmark.0.push(BenchmarkData {
        fps,
        cpu: 0.0,
        loaded_points: cell_stats.loaded_points,
        loaded_cells: loaded_cells.0.len() as u32,
        loading_cells: loading_cells.loading.len() as u32,
        should_load: loading_cells.should_load.len() as u32,
    });
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
