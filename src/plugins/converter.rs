use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use flume::{Receiver, Sender, TryRecvError};
use parking_lot::Mutex;

use point_converter::cell::{Cell, CellId};
use point_converter::converter::{add_points_to_cell, group_points};
use point_converter::metadata::{Metadata, MetadataConfig};
use point_converter::point::Point;

use crate::plugins::asset::source::{Directory, SourceError};
use crate::plugins::asset::{AssetLoadedEvent, AssetManagerRes, AssetManagerResMut, LoadAssetMsg};
use crate::plugins::thread_pool::ThreadPool;

pub struct ConverterPlugin;

impl Plugin for ConverterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FilesToConvert::default())
            .insert_resource(BatchedPointReader {
                reader: None,
                remaining_points: 0,
            })
            .insert_resource(CellTasks::default())
            .insert_state(ConversionState::NotStarted)
            .add_systems(
                OnEnter(ConversionState::Converting),
                (get_next_point_reader, get_point_batch).chain(),
            )
            .add_systems(
                Update,
                (receive_cell_tasks, add_points_to_cell_system)
                    .chain()
                    .run_if(in_state(ConversionState::Converting)),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States)]
enum ConversionState {
    NotStarted,
    Converting,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum FileConversionStatus {
    NotStarted,
    Converting,
    Finished,
    Failed,
}

#[derive(Debug)]
struct FileToConvert {
    path: PathBuf,
    status: FileConversionStatus,
}

#[derive(Debug, Default, Resource)]
struct FilesToConvert {
    current: usize,
    files: Vec<FileToConvert>,
}

#[derive(Resource)]
struct BatchedPointReader {
    reader: Option<Arc<Mutex<Box<dyn point_converter::converter::BatchedPointReader + Send>>>>,
    remaining_points: u64,
}

fn get_next_point_reader(
    mut batched_point_reader: ResMut<BatchedPointReader>,
    files_to_convert: Res<FilesToConvert>,
) {
    let file_to_convert = &files_to_convert.files[files_to_convert.current];

    batched_point_reader.reader = point_converter::get_batched_point_reader(&file_to_convert.path)
        .map(|it| Arc::new(Mutex::new(it)));

    /*  batched_point_reader.remaining_points = batched_point_reader
    .reader
    .as_ref()
    .unwrap()
    .lock()
    .remaining_points();*/
}

fn get_point_batch(
    batched_point_reader: Res<BatchedPointReader>,
    thread_pool: Res<ThreadPool>,
    cell_manager: AssetManagerRes<Cell>,
    cell_tasks: Res<CellTasks>, //active_metadata: ActiveMetadata,
) {
    let reader = batched_point_reader.reader.as_ref().unwrap().clone();

    let metadata = Metadata::default(); // TODO active_metadata.get().unwrap();
    let config = metadata.config.clone();
    let task_sender = cell_tasks.task_sender.clone();
    let load_sender = cell_manager.load_sender().clone();

    // TODO real source
    let working_directory = Directory::Path(PathBuf::from(
        "C:/Users/Julian/RustroverProjects/point-cloud/clouds/usc_converted2",
    ));

    thread_pool.execute(move || match reader.lock().get_batch(10_000) {
        Ok(points) => {
            if points.is_empty() {
                // TODO
            } else {
                let grouped_points = group_points(points, 0, &config);
                for (cell_index, points) in grouped_points {
                    let id = CellId {
                        index: cell_index,
                        hierarchy: 0,
                    };

                    let task = CellTask::new(id, points, &working_directory, &load_sender);
                    task_sender.send(task).unwrap();
                }
            }
        }
        Err(err) => {
            log::error!("{:?}", err);
        }
    });
}

#[derive(Debug)]
struct CellTask {
    points: Vec<Point>,
    loaded_asset_receiver: Receiver<AssetLoadedEvent<Cell>>,
}

impl CellTask {
    fn new(
        id: CellId,
        points: Vec<Point>,
        working_directory: &Directory,
        load_sender: &Sender<LoadAssetMsg<Cell>>,
    ) -> Self {
        let (sender, receiver) = flume::bounded(1);

        load_sender
            .send(LoadAssetMsg {
                id,
                source: working_directory.join(&id.path()),
                reply_sender: Some(sender),
            })
            .unwrap();

        CellTask {
            points,
            loaded_asset_receiver: receiver,
        }
    }
}

#[derive(Debug, Resource)]
struct CellTasks {
    tasks: VecDeque<CellTask>,
    task_sender: Sender<CellTask>,
    task_receiver: Receiver<CellTask>,
}

impl Default for CellTasks {
    fn default() -> Self {
        let (task_sender, task_receiver) = flume::unbounded();

        Self {
            tasks: VecDeque::default(),
            task_sender,
            task_receiver,
        }
    }
}

fn receive_cell_tasks(mut cell_tasks: ResMut<CellTasks>) {
    loop {
        match cell_tasks.task_receiver.try_recv() {
            Ok(task) => {
                cell_tasks.tasks.push_back(task);
            }
            Err(TryRecvError::Empty) => {
                break;
            }
            Err(TryRecvError::Disconnected) => {
                unreachable!("self always holds a sender")
            }
        }
    }
}

fn add_points_to_cell_system(
    mut cell_manager: AssetManagerResMut<Cell>,
    mut cell_tasks: ResMut<CellTasks>,
) {
    for _ in 0..cell_tasks.tasks.len().min(10) {
        if let Some(task) = cell_tasks.tasks.pop_front() {
            let config = MetadataConfig::default(); // TODO real config and source
            let working_directory = Directory::Path(PathBuf::from(
                "C:/Users/Julian/RustroverProjects/point-cloud/clouds/usc_converted2",
            ));

            match task.loaded_asset_receiver.try_recv() {
                Ok(loaded_asset_event) => {
                    let (id, remaining_points) = match loaded_asset_event {
                        AssetLoadedEvent::Success { handle } => {
                            log::debug!(
                                "Adding {} points to loaded cell {:?}",
                                task.points.len(),
                                handle.id()
                            );

                            let mut cell = cell_manager.get_mut(&handle).asset_mut();
                            (
                                *handle.id(),
                                add_points_to_cell(&config, task.points, &mut cell),
                            )
                        }
                        AssetLoadedEvent::Error { id, error } => {
                            match error {
                                SourceError::NotFound(_) => {
                                    // OK
                                }
                                _ => {
                                    // TODO do something with the failed cell
                                    log::error!("Failed to load cell {:?}: {:?}", id, error);
                                    continue;
                                }
                            }

                            log::debug!("Adding {} points to new cell {:?}", task.points.len(), id);

                            let cell_size = config.cell_size(id.hierarchy);
                            let cell_pos = config.cell_pos(id.index, cell_size);
                            let mut cell = Cell::new(
                                id,
                                config.sub_grid_dimension,
                                cell_size,
                                cell_pos,
                                10_000,
                            );

                            let remaining_points =
                                add_points_to_cell(&config, task.points, &mut cell);

                            let source = working_directory.join(&id.path());

                            let _handle = cell_manager.insert(id, cell, source); // TODO save handle?

                            (id, remaining_points)
                        }
                    };

                    for (cell_index, points) in remaining_points {
                        let id = CellId {
                            hierarchy: id.hierarchy + 1,
                            index: cell_index,
                        };

                        let task = CellTask::new(
                            id,
                            points,
                            &working_directory,
                            cell_manager.load_sender(),
                        );

                        cell_tasks.tasks.push_back(task);
                    }
                }
                Err(TryRecvError::Empty) => {
                    cell_tasks.tasks.push_back(task);
                }
                Err(TryRecvError::Disconnected) => {}
            }
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    if ui.button("Choose files to convert...").clicked() {
        select_files(world);
    }

    let mut params = SystemState::<(
        Res<FilesToConvert>,
        Res<State<ConversionState>>,
        ResMut<NextState<ConversionState>>,
    )>::new(world);

    let (files_to_convert, conversion_state, mut next_conversion_state) = params.get_mut(world);

    match conversion_state.get() {
        ConversionState::NotStarted => {
            let button = egui::Button::new("Start converting");
            if ui
                .add_enabled(!files_to_convert.files.is_empty(), button)
                .clicked()
            {
                next_conversion_state.set(ConversionState::Converting);
            }
        }
        ConversionState::Converting => {
            if ui.button("Stop converting").clicked() {
                next_conversion_state.set(ConversionState::NotStarted);
            }
        }
    }

    ui.collapsing("Files to convert", |ui| {
        list_files(ui, world);
    });
}

fn select_files(world: &mut World) {
    let files = {
        let window: &winit::window::Window = world
            .get_resource::<crate::plugins::winit::Window>()
            .unwrap();

        // TODO supported endings from variables
        rfd::FileDialog::new()
            .add_filter("points", &["las", "laz", "ply", Metadata::EXTENSION])
            .set_parent(window)
            .pick_files()
    };

    if let Some(files) = files {
        let mut files_to_convert = world.get_resource_mut::<FilesToConvert>().unwrap();
        files_to_convert.current = 0;
        files_to_convert.files = files
            .into_iter()
            .map(|file| FileToConvert {
                path: file,
                status: FileConversionStatus::NotStarted,
            })
            .collect();
    }
}

fn list_files(ui: &mut egui::Ui, world: &mut World) {
    let files_to_convert = world.get_resource::<FilesToConvert>().unwrap();

    let text_style = egui::TextStyle::Body;
    let row_height = ui.text_style_height(&text_style);

    egui::ScrollArea::vertical().show_rows(
        ui,
        row_height,
        files_to_convert.files.len(),
        |ui, row_range| {
            for row in row_range {
                let file_to_convert = &files_to_convert.files[row];

                let file_name = file_to_convert
                    .path
                    .file_name()
                    .and_then(|it| it.to_str())
                    .unwrap();

                let status = match file_to_convert.status {
                    FileConversionStatus::NotStarted => "⌛",
                    FileConversionStatus::Converting => "⏳",
                    FileConversionStatus::Finished => "✔",
                    FileConversionStatus::Failed => "✖",
                };

                ui.label(format!("{}\u{00A0}{}", status, file_name));
            }
        },
    );
}
