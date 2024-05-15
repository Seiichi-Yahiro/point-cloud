pub use app::App;

mod app;
mod event_set;
mod plugins;
mod texture;
mod transform;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn run() {
    App::run().await
}
