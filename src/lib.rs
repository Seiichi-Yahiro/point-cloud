pub use app::App;

mod app;
mod camera;
mod fps;
mod gpu;
mod input_data;
mod point_renderer;
mod texture;
mod transform;
mod ui;
mod viewport;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn run() {
    App::run().await
}
