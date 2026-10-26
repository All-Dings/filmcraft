//! Small host bridge: prepare a browser File without adding a project item, or relink one.
//! No host-specific references, network requests or project format changes.
use filmcraft_engine::Services;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

async fn prepare(file: &JsValue) -> Result<String, JsValue> {
    use wasm_bindgen::JsCast;
    let (name, source) = if let Some(blob) = file.dyn_ref::<web_sys::File>() {
        (blob.name(), crate::host_source::Source::Blob(blob.clone().into()))
    } else {
        let name = js_sys::Reflect::get(file, &"name".into())?.as_string().ok_or("Missing media name")?;
        let size = js_sys::Reflect::get(file, &"size".into())?.as_f64().ok_or("Missing media size")?;
        if !size.is_finite() || size < 1.0 || size.fract() != 0.0 || size > 9007199254740991.0 {
            return Err(js_sys::Error::new("Invalid media size").into());
        }
        let read = js_sys::Reflect::get(file, &"read".into())?.dyn_into::<js_sys::Function>()?;
        (name, crate::host_source::Source::Host { size: size as u64, read })
    };
    if name.is_empty() || name.len() > 1024 {
        return Err(js_sys::Error::new("Invalid media name").into());
    }
    let size = source.size();
    let path = crate::fs::register_source(&name, source);
    let span = filmcraft_engine::relink::FINGERPRINT_SPAN as u64;
    crate::fs::prefetch(&path, 0, size.min(span)).await;
    crate::fs::prefetch(&path, size.saturating_sub(span), size).await;
    crate::import::prewarm(&path, size).await;
    // Surface rejected/truncated host reads even when only preparing a path.
    for offset in [0, size.saturating_sub(1)] {
        crate::fs::WebServices.read_range(&path, offset, 1).map_err(|e| js_sys::Error::new(&e.to_string()))?;
    }
    Ok(path)
}

pub fn install(obj: &js_sys::Object) -> Result<(), JsValue> {
    let function = Closure::<dyn Fn(JsValue, JsValue) -> js_sys::Promise>::new(|file: JsValue, params: JsValue| {
        let params = crate::api::from_js(&params);
        future_to_promise(async move {
            if !params.is_object() || (params.get("item").is_some() && params.get("item").and_then(Value::as_u64).is_none()) {
                return Err(js_sys::Error::new("Expected options object with an unsigned item id").into());
            }
            let (guard, bin) = crate::import::on_ui(|w, _| {
                (
                    w.app.session.execute("project.inspect", json!({})).map(|v| json!([v["path"], v["name"], v["revision"]])).map_err(|e| e.to_string()),
                    w.app.import_bin().0,
                )
            })
            .await;
            let guard = guard.map_err(|e| js_sys::Error::new(&e))?;
            let path = prepare(&file).await?;
            let importing = params.get("import").and_then(Value::as_bool) == Some(true);
            if !importing && params.get("item").is_none() {
                return Ok(crate::api::to_js(&json!({"path": path})));
            }
            let mut params = params;
            let command = if importing {
                params = json!({"paths":[path], "bin":bin});
                "file.import"
            } else {
                params["path"] = json!(path);
                // Never remap unrelated items, force a mismatch, or move clip source times.
                params["relinkOthers"] = json!(false);
                params["alignTimecode"] = json!(false);
                params["force"] = json!(false);
                "media.relink"
            };
            for _ in 0..2000 {
                let p = params.clone();
                let guard = guard.clone();
                let (result, pending) = crate::import::on_ui(move |w, _| {
                    let current = w.app.session.execute("project.inspect", json!({})).map(|v| json!([v["path"], v["name"], v["revision"]]));
                    if current.ok().as_ref() != Some(&guard) {
                        return (Err("Project changed while preparing media".to_string()), false);
                    }
                    let _ = filmcraft_media::pending::take();
                    let result = w.app.session.execute(command, p).map_err(|e| e.to_string());
                    (result, filmcraft_media::pending::take())
                })
                .await;
                if pending && result.as_ref().is_ok_and(|v| v.get("errors").and_then(Value::as_array).is_some_and(|e| !e.is_empty())) {
                    crate::fs::idle().await;
                    continue;
                }
                match result {
                    Ok(result) => return Ok(crate::api::to_js(&result)),
                    Err(_) if pending => crate::fs::idle().await,
                    Err(error) => return Err(js_sys::Error::new(&error).into()),
                }
            }
            Err(js_sys::Error::new("Timed out loading relink media").into())
        })
    });
    js_sys::Reflect::set(obj, &"provideMedia".into(), &function.into_js_value()).map(|_| ())
}
