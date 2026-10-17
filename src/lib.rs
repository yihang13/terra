mod app;
mod gl;
mod math;
mod shaders;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| {
        let message = format!("Terra renderer panic: {info}");
        web_sys::console::error_1(&JsValue::from_str(&message));
    }));

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No browser window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("No document available"))?;

    let setup = (|| -> Result<WebGl2RenderingContext, JsValue> {
        let canvas = document
            .get_element_by_id("globe")
            .ok_or_else(|| JsValue::from_str("Canvas element #globe was not found"))?
            .dyn_into::<HtmlCanvasElement>()?;
        let context = canvas
            .get_context("webgl2")?
            .ok_or_else(|| JsValue::from_str("WebGL2 is not available in this browser"))?;
        let context: WebGl2RenderingContext = context.dyn_into()?;
        Ok(context)
    })();

    let gl = match setup {
        Ok(gl) => gl,
        Err(error) => {
            let message = error.as_string().unwrap_or_else(|| format!("{error:?}"));
            app::show_fatal(&message);
            return Ok(());
        }
    };

    wasm_bindgen_futures::spawn_local(async move {
        match app::create(gl).await {
            Ok(application) => app::begin(application),
            Err(error) => {
                let message = error.as_string().unwrap_or_else(|| format!("{error:?}"));
                app::show_fatal(&format!(
                    "Unable to initialise the orbital renderer: {message}"
                ));
            }
        }
    });

    Ok(())
}
