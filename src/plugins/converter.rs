use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemId, SystemState};
use bevy_state::prelude::*;
use caches::{Cache, LRUCache};
use flume::{Receiver, TryRecvError};
use parking_lot::Mutex;
use rustc_hash::{FxBuildHasher, FxHashMap};
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
use crate::plugins::metadata::{
    ActiveMetadata, LoadedMetadata, MetadataState, UpdateMetadataEvent,
};
use crate::plugins::thread_pool::ThreadPool;

pub struct ConverterPlugin;

impl Plugin for ConverterPlugin {
    fn build(&self, app: &mut App) {
        let next_file_system_id = app.world_mut().register_system(next_file);
        let read_batch_system_id = app.world_mut().register_system(read_batch);

        app.insert_resource(FilesToConvert {
            next_file: next_file_system_id,
            read_batch: read_batch_system_id,
            current: 0,
            files: vec![],
        })
        .insert_resource(PointBatchReceiver(None))
        .insert_resource(PointReader(None))
        .insert_resource(Tasks::default())
        .insert_resource(Settings::default())
        .insert_resource(CellCache::default())
        .insert_state(ConversionState::NotStarted)
        .add_systems(OnEnter(ConversionState::Converting), next_file)
        .add_systems(
            Update,
            (
                receive_tasks,
                get_handles_for_new_tasks,
                get_handles_for_loading_tasks,
                add_points_to_cell_system,
                handle_added_points_for_converting_file,
                check_if_tasks_are_finished,
            )
                .chain()
                .run_if(in_state(ConversionState::Converting)),
        )
        .add_systems(
            OnExit(ConversionState::Converting),
            (save, clear_cache)
                .chain()
                .run_if(|settings: Res<Settings>| settings.auto_save),
        )
        .add_systems(OnEnter(MetadataState::Loaded), disable_auto_save);
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

impl FileToConvert {
    fn create_reader(&self) -> Option<Box<dyn BatchedPointReader + Send>> {
        point_converter::get_batched_point_reader(&self.path)
    }
}

#[derive(Debug, Resource)]
struct FilesToConvert {
    next_file: SystemId,
    read_batch: SystemId,
    current: usize,
    files: Vec<FileToConvert>,
}

impl FilesToConvert {
    fn current_mut(&mut self) -> &mut FileToConvert {
        &mut self.files[self.current - 1]
    }

    fn next(&mut self) -> bool {
        self.current += 1;
        self.current <= self.files.len()
    }
}

#[derive(Resource)]
struct PointReader(Option<Arc<Mutex<Box<dyn BatchedPointReader + Send>>>>);

fn next_file(
    mut commands: Commands,
    mut files_to_convert: ResMut<FilesToConvert>,
    mut point_reader: ResMut<PointReader>,
    mut next_conversion_state: ResMut<NextState<ConversionState>>,
) {
    loop {
        if files_to_convert.next() {
            let current_file = files_to_convert.current_mut();

            if let Some(reader) = current_file.create_reader() {
                let total_points = reader.total_points();

                point_reader.0 = Some(Arc::new(Mutex::new(reader)));

                current_file.status = FileConversionStatus::Converting {
                    total: total_points,
                    remaining: total_points,
                };

                commands.run_system(files_to_convert.read_batch);

                break;
            } else {
                let msg = "File type not supported";
                let error = std::io::Error::new(std::io::ErrorKind::Unsupported, msg);

                current_file.status = FileConversionStatus::Failed {
                    error,
                    total: 0,
                    remaining: 0,
                };
            }
        } else {
            point_reader.0 = None;
            next_conversion_state.set(ConversionState::Finished);
            break;
        }
    }
}

fn read_batch(
    mut commands: Commands,
    point_reader: Res<PointReader>,
    thread_pool: Res<ThreadPool>,
    mut point_batch_receiver: ResMut<PointBatchReceiver>,
    active_metadata: ActiveMetadata,
    mut files_to_convert: ResMut<FilesToConvert>,
) {
    let Some(reader) = &point_reader.0 else {
        return;
    };

    let remaining_points = reader.lock().remaining_points();
    if remaining_points == 0 {
        let file_to_convert = files_to_convert.current_mut();
        file_to_convert.status = FileConversionStatus::Finished;

        commands.run_system(files_to_convert.next_file);
        return;
    }

    let reader = Arc::clone(reader);
    let config = active_metadata.get().config.clone();

    let (sender, receiver) = flume::bounded(1);
    point_batch_receiver.0 = Some(receiver);

    thread_pool.execute(move || {
        let result = reader.lock().get_batch(500_000).map(|points| {
            let aabb = Aabb::from(points.iter().map(|point| point.pos)).unwrap();
            let grouped_points = group_points(points, 0, &config);

            let tasks = grouped_points
                .into_iter()
                .map(|(cell_index, points)| {
                    let id = CellId {
                        index: cell_index,
                        hierarchy: 0,
                    };

                    CellTask { id, points }
                })
                .collect();

            PointBatch { aabb, tasks }
        });

        sender.send(result).unwrap();
    });
}

fn check_if_tasks_are_finished(
    mut commands: Commands,
    point_batch_receiver: Res<PointBatchReceiver>,
    files_to_convert: Res<FilesToConvert>,
    tasks: Res<Tasks>,
) {
    if point_batch_receiver.0.is_none()
        && tasks.new_tasks.is_empty()
        && tasks.tasks_with_loading_handle.is_empty()
        && tasks.tasks_with_handle.is_empty()
    {
        commands.run_system(files_to_convert.read_batch);
    }
}

#[derive(Debug)]
struct PointBatch {
    aabb: Aabb,
    tasks: Vec<CellTask>,
}

#[derive(Debug)]
struct CellTask {
    id: CellId,
    points: Vec<Point>,
}

#[derive(Resource)]
struct PointBatchReceiver(Option<Receiver<Result<PointBatch, std::io::Error>>>);

#[derive(Debug, Resource)]
struct Tasks {
    new_tasks: VecDeque<CellTask>,
    tasks_with_handle: VecDeque<(CellTask, AssetHandle<Cell>)>,
    tasks_with_loading_handle: VecDeque<(CellTask, Receiver<AssetLoadedEvent<Cell>>)>,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            new_tasks: VecDeque::default(),
            tasks_with_handle: VecDeque::default(),
            tasks_with_loading_handle: VecDeque::default(),
        }
    }
}

fn receive_tasks(
    mut commands: Commands,
    mut point_batch_receiver: ResMut<PointBatchReceiver>,
    mut tasks: ResMut<Tasks>,
    mut update_metadata: EventWriter<UpdateMetadataEvent>,
    mut files_to_convert: ResMut<FilesToConvert>,
) {
    let Some(receiver) = point_batch_receiver.0.take() else {
        return;
    };

    match receiver.try_recv() {
        Ok(result) => match result {
            Ok(point_batch) => {
                update_metadata.send(UpdateMetadataEvent::ExtendBoundingBox(point_batch.aabb));
                tasks.new_tasks.extend(point_batch.tasks);
            }
            Err(error) => {
                log::error!("{}", error);

                let file_to_convert = files_to_convert.current_mut();

                match file_to_convert.status {
                    FileConversionStatus::Converting { total, remaining } => {
                        file_to_convert.status = FileConversionStatus::Failed {
                            error,
                            total,
                            remaining,
                        };

                        commands.run_system(files_to_convert.next_file);
                    }
                    FileConversionStatus::NotStarted
                    | FileConversionStatus::Finished
                    | FileConversionStatus::Failed { .. } => {
                        unreachable!(
                            "Batches are only read from converting files: {:?}",
                            file_to_convert.status
                        );
                    }
                }
            }
        },
        Err(TryRecvError::Empty) => {
            point_batch_receiver.0 = Some(receiver);
        }
        Err(TryRecvError::Disconnected) => {
            log::error!("Point batch reader thread seems to have crashed");
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

fn save(
    mut metadata_manager: AssetManagerResMut<Metadata>,
    mut cell_manager: AssetManagerResMut<Cell>,
) {
    metadata_manager.save_all();
    cell_manager.save_all();
}

fn clear_cache(mut cell_cache: ResMut<CellCache>) {
    cell_cache.clear();
}

fn disable_auto_save(
    mut metadata_manager: AssetManagerResMut<Metadata>,
    mut cell_manager: AssetManagerResMut<Cell>,
    mut settings: ResMut<Settings>,
    mut cell_cache: ResMut<CellCache>,
) {
    settings.auto_save = false;
    cell_cache.convert_to_map();
    metadata_manager.set_auto_save(false);
    cell_manager.set_auto_save(false);
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

                        let handle = cell_manager.insert(id, cell, source, true);
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
    LRU(LRUCache<CellId, AssetHandle<Cell>, FxBuildHasher>),
    Map(FxHashMap<CellId, AssetHandle<Cell>>),
}

impl CellCache {
    fn iter(&self) -> Box<dyn Iterator<Item = (&CellId, &AssetHandle<Cell>)> + '_> {
        match self {
            CellCache::LRU(it) => Box::new(it.iter()),
            CellCache::Map(it) => Box::new(it.iter()),
        }
    }

    fn convert_to_lru(&mut self) {
        match self {
            CellCache::LRU(_) => {}
            CellCache::Map(it) => {
                let mut lru = LRUCache::with_hasher(100, FxBuildHasher).unwrap();

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
                let mut map = FxHashMap::with_capacity_and_hasher(it.cap() * 2, FxBuildHasher);

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

    fn clear(&mut self) {
        match self {
            CellCache::LRU(it) => it.purge(),
            CellCache::Map(it) => {
                it.clear();
            }
        }
    }
}

impl Default for CellCache {
    fn default() -> Self {
        Self::Map(FxHashMap::with_capacity_and_hasher(100, FxBuildHasher))
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
    let conversion_state = *world
        .get_resource::<State<ConversionState>>()
        .unwrap()
        .get();

    let is_converting = match conversion_state {
        ConversionState::NotStarted | ConversionState::Finished => false,
        ConversionState::Converting => true,
    };

    let new_point_cloud_button = egui::Button::new("New point cloud");

    if ui
        .add_enabled(!is_converting, new_point_cloud_button)
        .clicked()
    {
        let mut params = SystemState::<(
            ResMut<NextState<MetadataState>>,
            AssetManagerResMut<Metadata>,
        )>::new(world);

        let (mut next_metadata_state, mut metadata_manager) = params.get_mut(world);
        let _ = metadata_manager.insert(
            "Unknown".to_string(),
            Metadata::default(),
            Source::None,
            true,
        );
        next_metadata_state.set(MetadataState::Loading);
    }

    {
        let mut params = SystemState::<(
            ResMut<Settings>,
            ResMut<CellCache>,
            Res<LoadedMetadata>,
            AssetManagerResMut<Metadata>,
            AssetManagerResMut<Cell>,
        )>::new(world);

        if ui.button("Save at...").clicked() {
            let window: &winit::window::Window = world
                .get_resource::<crate::plugins::winit::Window>()
                .unwrap();

            let folder = rfd::FileDialog::new().set_parent(window).pick_folder();

            if let Some(folder) = folder {
                let (
                    mut settings,
                    mut cell_cache,
                    loaded_metadata,
                    mut metadata_manager,
                    mut cell_manager,
                ) = params.get_mut(world);

                let source = Source::Path(
                    folder
                        .join(Metadata::FILE_NAME)
                        .with_extension(Metadata::EXTENSION),
                );

                metadata_manager.set_source(loaded_metadata.get_active(), source);
                metadata_manager.set_auto_save(true);
                metadata_manager.save_all();

                for (id, handle) in cell_cache.iter() {
                    let source = Source::Path(folder.join(id.path()));
                    cell_manager.set_source(handle, source);
                }

                cell_manager.set_auto_save(true);
                cell_manager.save_all();

                cell_cache.convert_to_map();
                settings.auto_save = true;
            }
        }

        let (mut settings, mut cell_cache, loaded_metadata, mut metadata_manager, mut cell_manager) =
            params.get_mut(world);

        let mut auto_save = settings.auto_save;
        let checkbox = egui::Checkbox::new(&mut auto_save, "Auto save");

        let metadata_source = metadata_manager.get_asset_source(loaded_metadata.get_active());
        let auto_save_enabled = match metadata_source {
            Source::Path(_) => true,
            Source::URL(_) | Source::None => false,
        };

        if ui.add_enabled(auto_save_enabled, checkbox).changed() {
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

    let choose_files_to_convert_button = egui::Button::new("Choose files to convert...");

    if ui
        .add_enabled(!is_converting, choose_files_to_convert_button)
        .clicked()
    {
        select_files(world);
    }

    {
        let mut params =
            SystemState::<(Res<FilesToConvert>, ResMut<NextState<ConversionState>>)>::new(world);

        let (files_to_convert, mut next_conversion_state) = params.get_mut(world);

        match conversion_state {
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
        let mut params =
            SystemState::<(ResMut<FilesToConvert>, ResMut<NextState<ConversionState>>)>::new(world);
        let (mut files_to_convert, mut next_conversion_state) = params.get_mut(world);

        next_conversion_state.set(ConversionState::NotStarted);

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

                match &file_to_convert.status {
                    FileConversionStatus::NotStarted => {
                        ui.label(format!("⌛\u{00A0}{}", file_name));
                    }
                    FileConversionStatus::Converting { remaining, total } => {
                        ui.label(format!(
                            "⏳\u{00A0}{}\nRemaining: {}",
                            file_name,
                            remaining.separate_with_commas()
                        ))
                        .on_hover_text(format!("Total points: {}", total.separate_with_commas()));
                    }
                    FileConversionStatus::Finished => {
                        ui.label(format!("✔\u{00A0}{}", file_name));
                    }
                    FileConversionStatus::Failed {
                        error,
                        total,
                        remaining,
                    } => {
                        ui.label(format!("✖\u{00A0}{}", file_name))
                            .on_hover_text(format!(
                                "{}\nConverted points: {}/{}",
                                error,
                                (total - remaining).separate_with_commas(),
                                total.separate_with_commas()
                            ));
                    }
                };
            }
        },
    );
}
