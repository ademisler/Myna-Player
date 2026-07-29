use leptos::task::spawn_local;
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsCast, closure::Closure, prelude::*};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen(inline_js = r#"
export function mynaPlayerSubscribe(command, callback) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = callback;
  window.__TAURI__.core.invoke(command, { channel }).catch((error) => {
    console.error(`Myna Player channel ${command} failed`, error);
  });
  return channel;
}

export async function mynaPlayerInstallDragDrop(callback) {
  const current = window.__TAURI__.window.getCurrentWindow();
  const unlisten = await current.onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "over") {
      callback({ kind: "over", path: null });
    } else if (payload.type === "drop") {
      callback({ kind: "drop", path: payload.paths?.[0] ?? null });
    } else {
      callback({ kind: "leave", path: null });
    }
  });
  window.__mynaPlayerDragDropUnlisten = unlisten;
}

export function mynaPlayerInstallVideoSurfaceSync() {
  const sync = () => {
    const viewport = document.querySelector(".video-viewport");
    if (!viewport) return;
    const rect = viewport.getBoundingClientRect();
    window.__TAURI__.core.invoke("set_video_surface_rect", {
      rect: { x: rect.left, y: rect.top, width: rect.width, height: rect.height }
    }).catch((error) => console.error("Could not resize native video surface", error));
  };
  requestAnimationFrame(sync);
  const observer = new ResizeObserver(sync);
  const viewport = document.querySelector(".video-viewport");
  if (viewport) observer.observe(viewport);
  window.addEventListener("resize", sync);
  window.__mynaPlayerSurfaceObserver = observer;
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = mynaPlayerSubscribe)]
    fn subscribe_raw(command: &str, callback: &js_sys::Function) -> JsValue;

    #[wasm_bindgen(js_name = mynaPlayerInstallDragDrop)]
    fn install_drag_drop_raw(callback: &js_sys::Function) -> js_sys::Promise;

    #[wasm_bindgen(js_name = mynaPlayerInstallVideoSurfaceSync)]
    pub fn install_video_surface_sync();
}

#[derive(Serialize)]
pub struct EmptyArgs {}

pub async fn invoke_typed<T, A>(command: &str, args: &A) -> Result<T, String>
where
    T: DeserializeOwned,
    A: Serialize,
{
    let args = serde_wasm_bindgen::to_value(args).map_err(|error| error.to_string())?;
    let value = invoke(command, args).await.map_err(js_error)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub fn subscribe<T>(command: &'static str, callback: impl Fn(T) + 'static)
where
    T: DeserializeOwned + 'static,
{
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |value| {
        match serde_wasm_bindgen::from_value::<T>(value) {
            Ok(event) => callback(event),
            Err(error) => {
                web_sys::console::error_1(&format!("Invalid {command} event: {error}").into())
            }
        }
    });
    let handle = subscribe_raw(command, closure.as_ref().unchecked_ref());
    closure.forget();
    std::mem::forget(handle);
}

pub fn install_drag_drop<T>(callback: impl Fn(T) + 'static)
where
    T: DeserializeOwned + 'static,
{
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |value| {
        match serde_wasm_bindgen::from_value::<T>(value) {
            Ok(event) => callback(event),
            Err(error) => web_sys::console::error_1(&format!("Invalid drag event: {error}").into()),
        }
    });
    let promise = install_drag_drop_raw(closure.as_ref().unchecked_ref());
    closure.forget();
    spawn_local(async move {
        if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
            web_sys::console::error_1(
                &format!("Could not install native drag/drop: {}", js_error(error)).into(),
            );
        }
    });
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("Tauri command failed: {error:?}"))
}
