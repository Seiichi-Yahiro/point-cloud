use std::collections::VecDeque;
use std::hash::BuildHasherDefault;
use std::path::PathBuf;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use caches::{Cache, LRUCache};
use flume::{Receiver, Sender, TryRecvError};
use parking_lot::Mutex;
use rustc_hash::FxHasher;

use bounding_volume::Aabb;
use point_converter::cell::{Cell, CellId};
use point_converter::converter::{add_points_to_cell, group_points};
use point_converter::metadata::Metadata;
use point_converter::point::Point;

use crate::plugins::asset::source::{Source, SourceError};
use crate::plugins::asset::{
    AssetHandle, AssetLoadedEvent, AssetManagerRes, AssetManagerResMut, LoadAssetMsg,
};
use crate::plugins::metadata::{ActiveMetadata, MetadataState, UpdateMetadataEvent};
use crate::plugins::thread_pool::ThreadPool;

pub struct ConverterPlugin;

impl Plugin for ConverterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FilesToConvert::default())
            .insert_resource(BatchedPointReader {
                reader: None,
                remaining_points: 0,
            })
            .insert_resource(Tasks::default())
            .insert_resource(CellCache::default())
            .insert_state(ConversionState::NotStarted)
            .add_systems(
                OnEnter(ConversionState::Converting),
                (get_next_point_reader, get_point_batch).chain(),
            )
            .add_systems(
                Update,
                (
                    receive_tasks,
                    get_handles_for_new_tasks,
                    get_handles_for_loading_tasks,
                    add_points_to_cell_system,
                )
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
    tasks: Res<Tasks>,
    active_metadata: ActiveMetadata,
) {
    let reader = batched_point_reader.reader.as_ref().unwrap().clone();

    let config = active_metadata.get().config.clone();
    let task_sender = tasks.task_sender.clone();

    thread_pool.execute(move || loop {
        match reader.lock().get_batch(1_000_000) {
            Ok(points) => {
                if points.is_empty() {
                    break;
                } else {
                    let aabb = Aabb::from(points.iter().map(|point| point.pos)).unwrap();
                    task_sender.send(TaskMsg::UpdateBoundingBox(aabb)).unwrap();

                    let grouped_points = group_points(points, 0, &config);

                    for (cell_index, points) in grouped_points {
                        let id = CellId {
                            index: cell_index,
                            hierarchy: 0,
                        };

                        let cell_task = CellTask { id, points };

                        task_sender.send(TaskMsg::CellTask(cell_task)).unwrap();
                    }
                }
            }
            Err(err) => {
                log::error!("{:?}", err);
                break;
            }
        }
    });
}

#[derive(Debug)]
enum TaskMsg {
    UpdateBoundingBox(Aabb),
    CellTask(CellTask),
}

#[derive(Debug)]
struct CellTask {
    id: CellId,
    points: Vec<Point>,
}

#[derive(Debug, Resource)]
struct Tasks {
    new_tasks: VecDeque<CellTask>,
    tasks_with_handle: VecDeque<(CellTask, AssetHandle<Cell>)>,
    tasks_with_loading_handle: VecDeque<(CellTask, Receiver<AssetLoadedEvent<Cell>>)>,
    task_sender: Sender<TaskMsg>,
    task_receiver: Receiver<TaskMsg>,
}

impl Default for Tasks {
    fn default() -> Self {
        let (task_sender, task_receiver) = flume::bounded(25);

        Self {
            new_tasks: VecDeque::default(),
            tasks_with_handle: VecDeque::default(),
            tasks_with_loading_handle: VecDeque::default(),
            task_sender,
            task_receiver,
        }
    }
}

fn receive_tasks(mut tasks: ResMut<Tasks>, mut update_metadata: EventWriter<UpdateMetadataEvent>) {
    let free_spots = 10usize.saturating_sub(tasks.new_tasks.len());
    let mut i = 0;

    while i < free_spots {
        match tasks.task_receiver.try_recv() {
            Ok(TaskMsg::UpdateBoundingBox(aabb)) => {
                update_metadata.send(UpdateMetadataEvent::ExtendBoundingBox(aabb));
            }
            Ok(TaskMsg::CellTask(cell_task)) => {
                i += 1;
                tasks.new_tasks.push_back(cell_task);
            }
            Err(TryRecvError::Empty) => {
                break;
            }
            Err(TryRecvError::Disconnected) => {
                unreachable!("tasks always holds a sender")
            }
        }
    }
}

fn get_handles_for_new_tasks(
    active_metadata: ActiveMetadata,
    mut tasks: ResMut<Tasks>,
    mut cell_cache: ResMut<CellCache>,
    cell_manager: AssetManagerRes<Cell>,
) {
    let working_directory = active_metadata.get_working_directory();
    let free_loading_spots = 10usize.saturating_sub(tasks.tasks_with_loading_handle.len());
    let mut i = 0;

    while i < free_loading_spots {
        if let Some(cell_task) = tasks.new_tasks.pop_front() {
            if let Some(handle) = cell_cache.cache.get(&cell_task.id) {
                tasks
                    .tasks_with_handle
                    .push_back((cell_task, handle.clone()));
            } else {
                i += 1;

                let (sender, receiver) = flume::bounded(1);

                cell_manager
                    .load_sender()
                    .send(LoadAssetMsg {
                        id: cell_task.id,
                        source: working_directory
                            .as_ref()
                            .map_or(Source::None, |dir| dir.join(&cell_task.id.path())),
                        reply_sender: Some(sender),
                    })
                    .unwrap();

                tasks
                    .tasks_with_loading_handle
                    .push_back((cell_task, receiver));
            }
        } else {
            break;
        }
    }
}

fn get_handles_for_loading_tasks(
    mut cell_manager: AssetManagerResMut<Cell>,
    mut tasks: ResMut<Tasks>,
    mut cell_cache: ResMut<CellCache>,
    active_metadata: ActiveMetadata,
    mut update_metadata: EventWriter<UpdateMetadataEvent>,
) {
    let metadata = active_metadata.get();
    let working_directory = active_metadata.get_working_directory();

    for _ in 0..tasks.tasks_with_loading_handle.len().min(10) {
        if let Some((cell_task, receiver)) = tasks.tasks_with_loading_handle.pop_front() {
            match receiver.try_recv() {
                Ok(loaded_asset_event) => match loaded_asset_event {
                    AssetLoadedEvent::Success { handle } => {
                        cell_cache.cache.put(*handle.id(), handle.clone());
                        tasks.tasks_with_handle.push_back((cell_task, handle));
                    }
                    AssetLoadedEvent::Error { id, error } => {
                        match error {
                            SourceError::NotFound(_) | SourceError::NoSource => {
                                // OK
                            }
                            _ => {
                                // TODO do something with the failed cell
                                log::error!("Failed to load cell {:?}: {:?}", id, error);
                                continue;
                            }
                        }

                        log::debug!("Creating new cell {:?}", id);

                        update_metadata.send(UpdateMetadataEvent::IncreaseHierarchy(id.hierarchy));

                        let cell_size = metadata.config.cell_size(id.hierarchy);
                        let cell_pos = metadata.config.cell_pos(id.index, cell_size);
                        let cell = Cell::new(
                            id,
                            metadata.config.sub_grid_dimension,
                            cell_size,
                            cell_pos,
                            10_000,
                        );

                        let source = working_directory
                            .as_ref()
                            .map_or(Source::None, |dir| dir.join(&id.path()));

                        let handle = cell_manager.insert(id, cell, source);
                        cell_cache.cache.put(id, handle.clone());
                        tasks.tasks_with_handle.push_back((cell_task, handle));
                    }
                },
                Err(TryRecvError::Empty) => {
                    tasks
                        .tasks_with_loading_handle
                        .push_back((cell_task, receiver));
                    continue;
                }
                Err(TryRecvError::Disconnected) => {
                    unreachable!()
                }
            }
        }
    }
}

#[derive(Debug, Resource)]
struct CellCache {
    cache: LRUCache<CellId, AssetHandle<Cell>, BuildHasherDefault<FxHasher>>,
}

impl Default for CellCache {
    fn default() -> Self {
        Self {
            cache: LRUCache::with_hasher(100, BuildHasherDefault::default()).unwrap(),
        }
    }
}

fn add_points_to_cell_system(
    mut cell_manager: AssetManagerResMut<Cell>,
    mut tasks: ResMut<Tasks>,
    active_metadata: ActiveMetadata,
    mut update_metadata: EventWriter<UpdateMetadataEvent>,
) {
    let metadata = active_metadata.get();

    for _ in 0..tasks.tasks_with_handle.len().min(10) {
        if let Some((cell_task, handle)) = tasks.tasks_with_handle.pop_front() {
            let remaining_points = {
                let mut cell = cell_manager.get_asset_mut(&handle);
                let points_before = cell.header().total_number_of_points;

                let remaining_points =
                    add_points_to_cell(&metadata.config, cell_task.points, &mut cell);

                let points_after = cell.header().total_number_of_points;

                let added_points = points_after as i32 - points_before as i32;
                update_metadata.send(UpdateMetadataEvent::NumberOfPoints(added_points));

                log::debug!("Added {} points to cell {:?}", added_points, handle.id());

                remaining_points
            };

            for (cell_index, points) in remaining_points {
                let id = CellId {
                    hierarchy: handle.id().hierarchy + 1,
                    index: cell_index,
                };

                let task = CellTask { id, points };

                tasks.new_tasks.push_back(task);
            }
        }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    if ui.button("New point cloud").clicked() {
        let mut params = SystemState::<(
            ResMut<NextState<MetadataState>>,
            AssetManagerResMut<Metadata>,
        )>::new(world);

        let (mut next_metadata_state, mut metadata_manager) = params.get_mut(world);
        let _ = metadata_manager.insert(
            "Unknown".to_string(),
            Metadata::default(),
            Source::Path(std::path::PathBuf::from(
                "C:/Users/Julian/RustroverProjects/point-cloud/clouds/usc_converted2/metadata.json",
            )),
        );
        next_metadata_state.set(MetadataState::Loading);
    }

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
