use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/web.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    pub async fn chooseDir() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    pub async fn readBytes(
        dir: &web_sys::FileSystemDirectoryHandle,
        fileName: &str,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    pub async fn readCell(
        dir: &web_sys::FileSystemDirectoryHandle,
        hierarchy: &str,
        fileName: &str,
    ) -> Result<JsValue, JsValue>;
}
