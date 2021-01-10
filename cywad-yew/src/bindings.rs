use wasm_bindgen::prelude::*;

// wasm-bindgen will automatically take care of including this script
#[wasm_bindgen(module = "/src/sse.js")]
extern "C" {
    #[wasm_bindgen(js_name = "registerSseHandler")]
    pub fn register_sse_handler(
        sse_endpoint: String,
        on_sse_message: &Closure<dyn FnMut(String, String)>,
    );
}
