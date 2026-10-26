//! Optional asynchronous range reader supplied by an embedding host.
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[derive(Clone)]
pub enum Source {
    Blob(web_sys::Blob),
    Host { size: u64, read: js_sys::Function },
}
impl Source {
    pub fn size(&self) -> u64 {
        match self {
            Self::Blob(blob) => blob.size() as u64,
            Self::Host { size, .. } => *size,
        }
    }
    pub fn blob(&self) -> Option<web_sys::Blob> {
        match self {
            Self::Blob(blob) => Some(blob.clone()),
            Self::Host { .. } => None,
        }
    }
    pub async fn read(&self, start: u64, end: u64) -> Result<Vec<u8>, JsValue> {
        if start > end || end > self.size() || end - start > crate::fs::CHUNK {
            return Err(js_sys::Error::new("Invalid host read range").into());
        }
        let value = match self {
            Self::Blob(blob) => JsFuture::from(blob.slice_with_f64_and_f64(start as f64, end as f64)?.array_buffer()).await?,
            Self::Host { read, .. } => {
                let result = read.call2(&JsValue::NULL, &JsValue::from_f64(start as f64), &JsValue::from_f64(end as f64))?;
                JsFuture::from(js_sys::Promise::resolve(&result)).await?
            }
        };
        if !value.is_instance_of::<js_sys::ArrayBuffer>() && !value.is_instance_of::<js_sys::Uint8Array>() {
            return Err(js_sys::Error::new("Host reader must return ArrayBuffer or Uint8Array").into());
        }
        let length = js_sys::Reflect::get(&value, &"byteLength".into())?.as_f64();
        if length != Some((end - start) as f64) {
            return Err(js_sys::Error::new("Host reader returned the wrong byte count").into());
        }
        let bytes = js_sys::Uint8Array::new(&value);
        if u64::from(bytes.length()) != end - start {
            return Err(js_sys::Error::new("Host reader returned the wrong byte count").into());
        }
        Ok(bytes.to_vec())
    }
}
