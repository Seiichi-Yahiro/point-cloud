use std::ops::Deref;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use thread_pool::ThreadPool as InnerThreadPool;

pub struct ThreadPoolPlugin;

impl Plugin for ThreadPoolPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.insert_resource(ThreadPool::new(2));
        }

        #[cfg(target_arch = "wasm32")]
        {
            app.insert_non_send_resource(ThreadPool::new(2));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
#[derive(Debug, Clone)]
pub struct ThreadPool(Arc<InnerThreadPool>);

#[cfg(not(target_arch = "wasm32"))]
pub type ThreadPoolRes<'w> = Res<'w, ThreadPool>;

#[cfg(target_arch = "wasm32")]
pub type ThreadPoolRes<'w> = NonSend<'w, ThreadPool>;

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        Self(Arc::new(InnerThreadPool::new(size)))
    }
}

impl Deref for ThreadPool {
    type Target = Arc<InnerThreadPool>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
