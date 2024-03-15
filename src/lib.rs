pub use app::App;

mod app;
mod gpu;
mod viewport;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn run() {
    App::run().await
}
