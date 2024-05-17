use std::collections::VecDeque;
use std::hash::BuildHasherDefault;
use std::path::PathBuf;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemId, SystemState};
use caches::{Cache, LRUCache};
use flume::{Receiver, Sender, TryRecvError};
use rustc_hash::{FxHashMap, FxHasher};
use thousands::Separable;

use bounding_volume::Aabb;
use point_converter::cell::{Cell, CellId};
use point_converter::converter::{add_points_to_cell, group_points, BatchedPointReader};
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
        let spawn_point_reader_system_id = app.world.register_system(spawn_point_reader);

        app.insert_resource(FilesToConvert {
            spawn_point_reader: spawn_point_reader_system_id,
            current: 0,
            finished_reading: false,
            files: vec![],
        })
        .insert_resource(Tasks::default())
        .insert_resource(Settings::default())
        .insert_resource(CellCache::default())
        .insert_state(ConversionState::NotStarted)
        .add_systems(OnEnter(ConversionState::Converting), spawn_point_reader)
        .add_systems(
            Update,
            (
                receive_tasks,
                get_handles_for_new_tasks,
                get_handles_for_loading_tasks,
                add_points_to_cell_system,
                handle_added_points_for_converting_file,
                check_if_converting_file_is_finished,
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
    Finished,
}

#[derive(Debug)]
enum FileConversionStatus {
    NotStarted,
    Converting {
        total: u64,
        remaining: u64,
    },
    Finished,
    Failed {
        error: std::io::Error,
        total: u64,
        remaining: u64,
    },
}

#[derive(Debug)]
struct FileToConvert {
    path: PathBuf,
    status: FileConversionStatus,
}

#[derive(Debug, Resource)]
struct FilesToConvert {
    spawn_point_reader: SystemId,
    current: usize,
    finished_reading: bool,
    files: Vec<FileToConvert>,
}

impl FilesToConvert {
    fn current(&self) -> &FileToConvert {
        &self.files[self.current]
    }

    fn current_mut(&mut self) -> &mut FileToConvert {
        &mut self.files[self.current]
    }

    fn create_reader(&self) -> Option<Box<dyn BatchedPointReader + Send>> {
        point_converter::get_batched_point_reader(&self.current().path)
    }

    fn next(&mut self) -> bool {
        self.current += 1;
        self.finished_reading = false;
        self.current < self.files.len()
    }
}

fn spawn_point_reader(
    mut files_to_convert: ResMut<FilesToConvert>,
    thread_pool: Res<ThreadPool>,
    tasks: Res<Tasks>,
    active_metadata: ActiveMetadata,
) {
    let task_sender = tasks.task_sender.clone();

    let Some(mut reader) = files_to_convert.create_reader() else {
        let msg = "File type not supported";
        let error = std::io::Error::new(std::io::ErrorKind::Unsupported, msg);
        task_sender.send(TaskMsg::Failed(error)).unwrap();
        return;
    };

    let total_points = reader.total_points();

    files_to_convert.current_mut().status = FileConversionStatus::Converting {
        total: total_points,
        remaining: total_points,
    };

    let config = active_metadata.get().config.clone();

    thread_pool.execute(move || loop {
        match reader.get_batch(100_000) {
            Ok(points) => {
                if points.is_empty() {
                    task_sender.send(TaskMsg::Finished).unwrap();
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
                task_sender.send(TaskMsg::Failed(err)).unwrap();
                break;
            }
        }
    });
}

#[derive(Debug)]
enum TaskMsg {
    UpdateBoundingBox(Aabb),
    CellTask(CellTask),
    Failed(std::io::Error),
    Finished,
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

fn receive_tasks(
    mut tasks: ResMut<Tasks>,
    mut update_metadata: EventWriter<UpdateMetadataEvent>,
    mut files_to_convert: ResMut<FilesToConvert>,
) {
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
            Ok(TaskMsg::Finished) => {
                files_to_convert.finished_reading = true;
            }
            Ok(TaskMsg::Failed(error)) => {
                files_to_convert.finished_reading = true;

                let file_to_convert = files_to_convert.current_mut();

                match file_to_convert.status {
                    FileConversionStatus::NotStarted => {
                        file_to_convert.status = FileConversionStatus::Failed {
                            error,
                            total: 0,
                            remaining: 0,
                        };
                    }
                    FileConversionStatus::Converting { total, remaining } => {
                        file_to_convert.status = FileConversionStatus::Failed {
                            error,
                            total,
                            remaining,
                        };
                    }

                    FileConversionStatus::Finished => {
                        unreachable!("Finished files don't fail")
                    }
                    FileConversionStatus::Failed { .. } => {
                        unreachable!("Failed files already failed");
                    }
                }
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

fn handle_added_points_for_converting_file(
    mut events: EventReader<UpdateMetadataEvent>,
    mut files_to_convert: ResMut<FilesToConvert>,
) {
    for event in events.read() {
        if let UpdateMetadataEvent::NumberOfPoints(points) = event {
            match &mut files_to_convert.current_mut().status {
                FileConversionStatus::Converting { remaining, .. }
                | FileConversionStatus::Failed { remaining, .. } => {
                    *remaining = remaining.wrapping_add_signed(-*points as i64);
                }
                FileConversionStatus::NotStarted | FileConversionStatus::Finished => {
                    unreachable!("Only converting and failed files can receive points");
                }
            }
        }
    }
}

fn check_if_converting_file_is_finished(
    mut commands: Commands,
    mut next_conversion_state: ResMut<NextState<ConversionState>>,
    mut files_to_convert: ResMut<FilesToConvert>,
    tasks: Res<Tasks>,
) {
    if files_to_convert.finished_reading
        && tasks.new_tasks.is_empty()
        && tasks.tasks_with_loading_handle.is_empty()
        && tasks.tasks_with_handle.is_empty()
    {
        let file_to_convert = files_to_convert.current_mut();

        match file_to_convert.status {
            FileConversionStatus::Converting { .. } => {
                file_to_convert.status = FileConversionStatus::Finished;
            }
            FileConversionStatus::Failed { .. } => {}
            FileConversionStatus::NotStarted => {
                unreachable!();
            }
            FileConversionStatus::Finished => {
                unreachable!()
            }
        }

        if files_to_convert.next() {
            commands.run_system(files_to_convert.spawn_point_reader);
        } else {
            next_conversion_state.set(ConversionState::Finished);
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
            if let Some(handle) = cell_cache.get(&cell_task.id) {
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
                        cell_cache.insert(*handle.id(), handle.clone());
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
                        cell_cache.insert(id, handle.clone());
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
enum CellCache {
    LRU(LRUCache<CellId, AssetHandle<Cell>, BuildHasherDefault<FxHasher>>),
    Map(FxHashMap<CellId, AssetHandle<Cell>>),
}

impl CellCache {
    fn convert_to_lru(&mut self) {
        match self {
            CellCache::LRU(_) => {}
            CellCache::Map(it) => {
                let mut lru = LRUCache::with_hasher(100, BuildHasherDefault::default()).unwrap();

                for (id, handle) in it.drain().take(100) {
                    lru.put(id, handle);
                }

                *self = CellCache::LRU(lru);
            }
        }
    }

    fn convert_to_map(&mut self) {
        match self {
            CellCache::LRU(it) => {
                let mut map = FxHashMap::with_capacity_and_hasher(
                    it.cap() * 2,
                    BuildHasherDefault::default(),
                );

                while let Some((id, handle)) = it.remove_lru() {
                    map.insert(id, handle);
                }

                *self = CellCache::Map(map);
            }
            CellCache::Map(_) => {}
        }
    }

    fn insert(&mut self, id: CellId, handle: AssetHandle<Cell>) {
        match self {
            CellCache::LRU(it) => {
                it.put(id, handle);
            }
            CellCache::Map(it) => {
                it.insert(id, handle);
            }
        }
    }

    fn get(&mut self, id: &CellId) -> Option<&AssetHandle<Cell>> {
        match self {
            CellCache::LRU(it) => it.get(id),
            CellCache::Map(it) => it.get(id),
        }
    }
}

impl Default for CellCache {
    fn default() -> Self {
        Self::Map(FxHashMap::with_capacity_and_hasher(
            100,
            BuildHasherDefault::default(),
        ))
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

#[derive(Debug, Resource)]
struct Settings {
    auto_save: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_save: false }
    }
}

pub fn draw_ui(ui: &mut egui::Ui, world: &mut World) {
    if ui.button("New point cloud").clicked() {
        let mut params = SystemState::<(
            ResMut<NextState<MetadataState>>,
            AssetManagerResMut<Metadata>,
        )>::new(world);

        let (mut next_metadata_state, mut metadata_manager) = params.get_mut(world);
        let _ = metadata_manager.insert("Unknown".to_string(), Metadata::default(), Source::None);
        next_metadata_state.set(MetadataState::Loading);
    }

    {
        let mut params = SystemState::<(
            ResMut<Settings>,
            ResMut<CellCache>,
            AssetManagerResMut<Metadata>,
            AssetManagerResMut<Cell>,
        )>::new(world);

        let (mut settings, mut cell_cache, mut metadata_manager, mut cell_manager) =
            params.get_mut(world);

        let mut auto_save = settings.auto_save;
        if ui.checkbox(&mut auto_save, "Auto save").changed() {
            settings.auto_save = auto_save;
            metadata_manager.set_auto_save(auto_save);
            cell_manager.set_auto_save(auto_save);

            if auto_save {
                cell_cache.convert_to_lru();
            } else {
                cell_cache.convert_to_map();
            }
        }
    }

    ui.separator();

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
                next_conversion_state.set(ConversionState::Finished);
            }
        }
        ConversionState::Finished => {}
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

                let (status, remaining) = match &file_to_convert.status {
                    FileConversionStatus::NotStarted => ("⌛", None),
                    FileConversionStatus::Converting { remaining, .. } => ("⏳", Some(*remaining)),
                    FileConversionStatus::Finished => ("✔", None),
                    FileConversionStatus::Failed { remaining, .. } => ("✖", Some(*remaining)),
                };

                if let Some(remaining) = remaining {
                    ui.label(format!(
                        "{}\u{00A0}{}: {}",
                        status,
                        file_name,
                        remaining.separate_with_commas()
                    ));
                } else {
                    ui.label(format!("{}\u{00A0}{}", status, file_name));
                }
            }
        },
    );
}
