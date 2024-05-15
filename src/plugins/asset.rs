use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender, TryRecvError};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::plugins::asset::source::{Source, SourceError};
use crate::plugins::thread_pool::{ThreadPool, ThreadPoolRes};

pub mod source;

pub struct AssetPlugin<T>(std::marker::PhantomData<T>)
where
    T: Asset;

impl<T> Plugin for AssetPlugin<T>
where
    T: Asset,
{
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.insert_resource(AssetManager::<T>::default());
        }

        #[cfg(target_arch = "wasm32")]
        {
            app.insert_non_send_resource(AssetManager::<T>::default());
        }

        app.add_event::<AssetEvent<T>>()
            .add_systems(PreUpdate, handle_loaded_events::<T>)
            .add_systems(
                PostUpdate,
                (
                    (handle_load_events::<T>, handle_dropped_events::<T>).chain(),
                    send_created_and_changed_events::<T>,
                ),
            );
    }
}

impl<T> Default for AssetPlugin<T>
where
    T: Asset,
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

pub trait Asset: Send + Sync + Sized + 'static {
    type Id: Debug + Eq + Hash + Clone + Send + Sync;

    fn read_from(reader: &mut dyn Read) -> Result<Self, SourceError>;

    fn save(&self, _source: Source) -> Result<(), SourceError> {
        Ok(())
    }
}

#[derive(Debug, Component)]
pub struct AssetHandle<T>
where
    T: Asset,
{
    id: T::Id,
    ref_count_sender: Sender<ChangeRefCount<T::Id>>,
}

impl<T> AssetHandle<T>
where
    T: Asset,
{
    fn new(id: T::Id, ref_count_sender: Sender<ChangeRefCount<T::Id>>) -> Self {
        ref_count_sender
            .send(ChangeRefCount::Increase(id.clone()))
            .unwrap();

        Self {
            id,
            ref_count_sender,
        }
    }

    pub fn id(&self) -> &T::Id {
        &self.id
    }
}

impl<T> PartialEq for AssetHandle<T>
where
    T: Asset,
{
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<T> Eq for AssetHandle<T> where T: Asset {}

impl<T> Hash for AssetHandle<T>
where
    T: Asset,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Clone for AssetHandle<T>
where
    T: Asset,
{
    fn clone(&self) -> Self {
        self.ref_count_sender
            .send(ChangeRefCount::Increase(self.id.clone()))
            .unwrap();

        Self {
            id: self.id.clone(),
            ref_count_sender: self.ref_count_sender.clone(),
        }
    }
}

impl<T> Drop for AssetHandle<T>
where
    T: Asset,
{
    fn drop(&mut self) {
        let _ = self
            .ref_count_sender
            .send(ChangeRefCount::Decrease(self.id.clone()));
    }
}

#[derive(Debug)]
pub struct LoadAssetMsg<T>
where
    T: Asset,
{
    pub id: T::Id,
    pub source: Source,
    pub reply_sender: Option<Sender<AssetLoadedEvent<T>>>,
}

#[derive(Debug)]
struct LoadedAssetMsg<T>
where
    T: Asset,
{
    id: T::Id,
    asset: Result<T, SourceError>,
}

#[derive(Debug)]
pub enum AssetLoadedEvent<T>
where
    T: Asset,
{
    Success { handle: AssetHandle<T> },
    Error { id: T::Id, error: SourceError },
}

impl<T> Clone for AssetLoadedEvent<T>
where
    T: Asset,
{
    fn clone(&self) -> Self {
        match self {
            Self::Success { handle } => Self::Success {
                handle: handle.clone(),
            },
            Self::Error { id, error } => Self::Error {
                id: id.clone(),
                error: error.clone(),
            },
        }
    }
}

#[derive(Debug, Event)]
pub enum AssetEvent<T>
where
    T: Asset,
{
    Created { handle: AssetHandle<T> },
    Changed { handle: AssetHandle<T> },
    Loaded(AssetLoadedEvent<T>),
}

impl<T> Clone for AssetEvent<T>
where
    T: Asset,
{
    fn clone(&self) -> Self {
        match self {
            AssetEvent::Created { handle } => AssetEvent::Created {
                handle: handle.clone(),
            },
            AssetEvent::Changed { handle } => AssetEvent::Changed {
                handle: handle.clone(),
            },
            AssetEvent::Loaded(loaded) => AssetEvent::Loaded(loaded.clone()),
        }
    }
}

#[derive(Debug)]
struct Channels<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> Default for Channels<T> {
    fn default() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self { sender, receiver }
    }
}

#[derive(Debug)]
enum ChangeRefCount<T>
where
    T: Debug + Eq + Hash + Clone,
{
    Increase(T),
    Decrease(T),
}

#[derive(Debug)]
pub struct AssetEntry<T>
where
    T: Asset,
{
    source: Source,
    load_status: AssetLoadStatus,
    change_status: AssetChangeStatus,
    asset: Option<T>,
}

impl<T> AssetEntry<T>
where
    T: Asset,
{
    pub fn asset(&self) -> &T {
        self.asset.as_ref().unwrap()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

#[derive(Debug)]
pub struct MutAsset<'a, T>
where
    T: Asset,
{
    handle: AssetHandle<T>,
    asset: &'a mut T,
    change_status: &'a mut AssetChangeStatus,
    has_just_changed: bool,
    just_changed: &'a mut FxHashSet<AssetHandle<T>>,
}

impl<'a, T> Deref for MutAsset<'a, T>
where
    T: Asset,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.asset
    }
}

impl<'a, T> DerefMut for MutAsset<'a, T>
where
    T: Asset,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        *self.change_status = AssetChangeStatus::Changed;
        self.has_just_changed = true;
        self.asset
    }
}

impl<'a, T> Drop for MutAsset<'a, T>
where
    T: Asset,
{
    fn drop(&mut self) {
        if self.has_just_changed {
            self.just_changed.insert(self.handle.clone());
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum AssetLoadStatus {
    Loading,
    Loaded,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum AssetChangeStatus {
    UnChanged,
    Changed,
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
#[derive(Debug)]
pub struct AssetManager<T>
where
    T: Asset,
{
    store: FxHashMap<T::Id, AssetEntry<T>>,
    just_created: Vec<AssetHandle<T>>,
    just_changed: FxHashSet<AssetHandle<T>>,
    load_channels: Channels<LoadAssetMsg<T>>,
    loaded_channels: Channels<LoadedAssetMsg<T>>,
    ref_count_channels: Channels<ChangeRefCount<T::Id>>,
    ref_counts: FxHashMap<T::Id, u32>,
    waiting_for_reply: FxHashMap<T::Id, Vec<Sender<AssetLoadedEvent<T>>>>,
}

#[cfg(not(target_arch = "wasm32"))]
pub type AssetManagerRes<'w, T> = Res<'w, AssetManager<T>>;

#[cfg(target_arch = "wasm32")]
pub type AssetManagerRes<'w, T> = NonSend<'w, AssetManager<T>>;

#[cfg(not(target_arch = "wasm32"))]
pub type AssetManagerResMut<'w, T> = ResMut<'w, AssetManager<T>>;

#[cfg(target_arch = "wasm32")]
pub type AssetManagerResMut<'w, T> = NonSendMut<'w, AssetManager<T>>;

impl<T> Default for AssetManager<T>
where
    T: Asset,
{
    fn default() -> Self {
        Self {
            store: FxHashMap::default(),
            just_created: Vec::default(),
            just_changed: FxHashSet::default(),
            load_channels: Channels::default(),
            loaded_channels: Channels::default(),
            ref_count_channels: Channels::default(),
            ref_counts: FxHashMap::default(),
            waiting_for_reply: FxHashMap::default(),
        }
    }
}

impl<T> AssetManager<T>
where
    T: Asset,
{
    pub fn load_sender(&self) -> &Sender<LoadAssetMsg<T>> {
        &self.load_channels.sender
    }

    #[must_use]
    pub fn insert(&mut self, id: T::Id, asset: T, source: Source) -> AssetHandle<T> {
        self.store.insert(
            id.clone(),
            AssetEntry {
                source,
                load_status: AssetLoadStatus::Loaded,
                change_status: AssetChangeStatus::Changed,
                asset: Some(asset),
            },
        );

        self.ref_counts.entry(id.clone()).or_insert(0);

        let handle = AssetHandle::new(id, self.ref_count_channels.sender.clone());

        self.just_created.push(handle.clone());

        handle
    }

    pub fn get_asset(&self, handle: &AssetHandle<T>) -> &T {
        self.store.get(handle.id()).unwrap().asset.as_ref().unwrap()
    }

    pub fn get_asset_mut(&mut self, handle: &AssetHandle<T>) -> MutAsset<T> {
        let entry = self.store.get_mut(handle.id()).unwrap();

        MutAsset {
            handle: handle.clone(),
            asset: entry.asset.as_mut().unwrap(),
            change_status: &mut entry.change_status,
            has_just_changed: false,
            just_changed: &mut self.just_changed,
        }
    }

    pub fn get_asset_source(&self, handle: &AssetHandle<T>) -> &Source {
        &self.store.get(handle.id()).unwrap().source
    }

    fn handle_load_events(
        &mut self,
        event_writer: &mut EventWriter<AssetEvent<T>>,
        thread_pool: &ThreadPool,
    ) {
        loop {
            match self.load_channels.receiver.try_recv() {
                Ok(msg) => match self.store.entry(msg.id.clone()) {
                    Entry::Occupied(entry) => match entry.get().load_status {
                        AssetLoadStatus::Loading => {
                            if let Some(sender) = msg.reply_sender {
                                self.waiting_for_reply
                                    .entry(msg.id.clone())
                                    .or_default()
                                    .push(sender);
                            }
                        }
                        AssetLoadStatus::Loaded => {
                            let handle =
                                AssetHandle::new(msg.id, self.ref_count_channels.sender.clone());

                            let asset_loaded_event = AssetLoadedEvent::Success { handle };
                            event_writer.send(AssetEvent::Loaded(asset_loaded_event.clone()));

                            if let Some(sender) = msg.reply_sender {
                                let _ = sender.send(asset_loaded_event);
                            }
                        }
                    },
                    Entry::Vacant(entry) => {
                        entry.insert(AssetEntry {
                            source: msg.source.clone(),
                            load_status: AssetLoadStatus::Loading,
                            change_status: AssetChangeStatus::UnChanged,
                            asset: None,
                        });

                        if let Some(sender) = msg.reply_sender {
                            self.waiting_for_reply
                                .entry(msg.id.clone())
                                .or_default()
                                .push(sender);
                        }

                        let loaded_sender = self.loaded_channels.sender.clone();
                        let id = msg.id;
                        let source = msg.source;

                        #[cfg(not(target_arch = "wasm32"))]
                        thread_pool.execute(move || {
                            let asset = source.load();
                            loaded_sender.send(LoadedAssetMsg { id, asset }).unwrap();
                        });

                        #[cfg(target_arch = "wasm32")]
                        thread_pool.execute_async(async move {
                            let asset = source.load().await;
                            loaded_sender.send(LoadedAssetMsg { id, asset }).unwrap();
                        });
                    }
                },
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    unreachable!("self always holds a sender")
                }
            }
        }
    }

    fn handle_loaded_events(&mut self, event_writer: &mut EventWriter<AssetEvent<T>>) {
        loop {
            match self.loaded_channels.receiver.try_recv() {
                Ok(msg) => {
                    let waiting_for_reply =
                        self.waiting_for_reply.remove(&msg.id).unwrap_or_default();

                    let Entry::Occupied(mut asset_status_entry) = self.store.entry(msg.id.clone())
                    else {
                        panic!("Asset entry should have been created for loading");
                    };

                    match msg.asset {
                        Ok(asset) => {
                            let entry = asset_status_entry.get_mut();
                            entry.asset = Some(asset);
                            entry.load_status = AssetLoadStatus::Loaded;

                            self.ref_counts.insert(msg.id.clone(), 0);

                            let asset_loaded_event = AssetLoadedEvent::Success {
                                handle: AssetHandle::new(
                                    msg.id.clone(),
                                    self.ref_count_channels.sender.clone(),
                                ),
                            };

                            for sender in waiting_for_reply {
                                let _ = sender.send(asset_loaded_event.clone());
                            }

                            event_writer.send(AssetEvent::Loaded(asset_loaded_event));
                        }
                        Err(err) => {
                            asset_status_entry.remove();

                            let asset_loaded_event = AssetLoadedEvent::Error {
                                id: msg.id.clone(),
                                error: err.into(),
                            };

                            for sender in waiting_for_reply {
                                let _ = sender.send(asset_loaded_event.clone());
                            }

                            event_writer.send(AssetEvent::Loaded(asset_loaded_event));
                        }
                    }
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

    fn send_created_events(&mut self, event_writer: &mut EventWriter<AssetEvent<T>>) {
        event_writer.send_batch(
            self.just_created
                .drain(..)
                .map(|handle| AssetEvent::Created { handle }),
        );
    }

    fn send_changed_events(&mut self, event_writer: &mut EventWriter<AssetEvent<T>>) {
        event_writer.send_batch(
            self.just_changed
                .drain()
                .map(|handle| AssetEvent::Changed { handle }),
        );
    }

    fn handle_ref_count_events(&mut self) {
        let mut freed_assets = FxHashSet::default();

        loop {
            match self.ref_count_channels.receiver.try_recv() {
                Ok(changed_ref_count) => match changed_ref_count {
                    ChangeRefCount::Increase(id) => {
                        *self.ref_counts.get_mut(&id).unwrap() += 1;
                        freed_assets.remove(&id);
                    }
                    ChangeRefCount::Decrease(id) => {
                        let ref_counts = self.ref_counts.get_mut(&id).unwrap();
                        *ref_counts -= 1;

                        if *ref_counts == 0 {
                            freed_assets.insert(id);
                        }
                    }
                },
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    unreachable!("self always holds a sender")
                }
            }
        }

        for id in freed_assets.drain() {
            log::debug!("Evicting asset {:?}", id);

            let entry = self.store.remove(&id).unwrap();
            match entry.load_status {
                AssetLoadStatus::Loading => {}
                AssetLoadStatus::Loaded => {
                    let asset = entry.asset.unwrap();

                    if entry.change_status == AssetChangeStatus::Changed {
                        asset.save(entry.source).unwrap(); // TODO thread?
                    }
                }
            }
        }
    }
}

fn handle_load_events<T: Asset>(
    mut asset_manager: AssetManagerResMut<T>,
    mut asset_events: EventWriter<AssetEvent<T>>,
    thread_pool: ThreadPoolRes,
) {
    asset_manager.handle_load_events(&mut asset_events, &thread_pool);
}

fn handle_loaded_events<T: Asset>(
    mut asset_manager: AssetManagerResMut<T>,
    mut asset_events: EventWriter<AssetEvent<T>>,
) {
    asset_manager.handle_loaded_events(&mut asset_events);
}

fn send_created_and_changed_events<T: Asset>(
    mut asset_manager: AssetManagerResMut<T>,
    mut asset_events: EventWriter<AssetEvent<T>>,
) {
    asset_manager.send_created_events(&mut asset_events);
    asset_manager.send_changed_events(&mut asset_events);
}

fn handle_dropped_events<T: Asset>(mut asset_manager: AssetManagerResMut<T>) {
    asset_manager.handle_ref_count_events();
}
