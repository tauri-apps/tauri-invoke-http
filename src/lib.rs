// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{
  collections::HashMap,
  str::FromStr,
  sync::{Arc, Mutex},
};

use tauri::{
  ipc::{CallbackFn, InvokeBody, InvokeResponse},
  webview::InvokeRequest,
  AppHandle, Manager, Runtime, Url,
};
use tiny_http::{Header, Method, Request, Response};

fn cors<R: std::io::Read>(request: &Request, r: &mut Response<R>, allowed_origins: &[String]) {
  if allowed_origins.iter().any(|s| s == "*") {
    r.add_header(Header::from_str("Access-Control-Allow-Origin: *").unwrap());
  } else if let Some(origin) = request.headers().iter().find(|h| h.field.equiv("Origin")) {
    if allowed_origins.iter().any(|o| o == &origin.value) {
      r.add_header(
        Header::from_str(&format!("Access-Control-Allow-Origin: {}", origin.value)).unwrap(),
      );
    }
  }
  r.add_header(Header::from_str("Access-Control-Expose-Headers: Tauri-Response").unwrap());
  r.add_header(Header::from_str("Access-Control-Allow-Headers: *").unwrap());
  r.add_header(Header::from_str("Access-Control-Allow-Methods: POST, OPTIONS").unwrap());
}

pub struct Invoke {
  allowed_origins: Vec<String>,
  port: u16,
  requests: Arc<Mutex<HashMap<u32, Request>>>,
}

impl Invoke {
  pub fn new<I: Into<String>, O: IntoIterator<Item = I>>(allowed_origins: O) -> Self {
    let port = portpicker::pick_unused_port().expect("failed to get unused port for invoke");
    let requests = Arc::new(Mutex::new(HashMap::new()));
    Self {
      allowed_origins: allowed_origins.into_iter().map(|o| o.into()).collect(),
      port,
      requests,
    }
  }

  pub fn start<R: Runtime>(&self, app: &AppHandle<R>) {
    let app = app.clone();
    let server = tiny_http::Server::http(format!("localhost:{}", self.port)).unwrap();
    let requests = self.requests.clone();
    let allowed_origins = self.allowed_origins.clone();
    std::thread::spawn(move || {
      for mut request in server.incoming_requests() {
        if request.method() == &Method::Options {
          let mut r = Response::empty(200u16);
          cors(&request, &mut r, &allowed_origins);
          request.respond(r).unwrap();
          continue;
        }
        let url = request.url().to_string();
        let pieces = url.split('/').collect::<Vec<_>>();
        let window_label = pieces[1];
        let cmd = pieces[2];

        if let Some(window) = app.get_webview_window(window_label) {
          let content_type = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Content-Type"))
            .map(|h| h.value.to_string())
            .unwrap_or_else(|| "application/json".into());

          let origin = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Origin"))
            .map(|h| h.value.to_string())
            .expect("Invalid IPC request - No Origin");
          let invoke_key = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Tauri-Invoke-Key"))
            .map(|h| h.value.to_string())
            .expect("Invalid IPC request - No Tauri-Invoke-Key");
          let callback = CallbackFn(
            request
              .headers()
              .iter()
              .find(|h| h.field.equiv("Tauri-Callback"))
              .map(|h| h.value.to_string())
              .expect("Invalid IPC request - No Tauri-Callback")
              .parse()
              .unwrap(),
          );
          let error = CallbackFn(
            request
              .headers()
              .iter()
              .find(|h| h.field.equiv("Tauri-Error"))
              .map(|h| h.value.to_string())
              .expect("Invalid IPC request - No Tauri-Error")
              .parse()
              .unwrap(),
          );

          let headers = (&request
            .headers()
            .iter()
            .map(|h| (h.field.to_string(), h.value.to_string()))
            .collect::<HashMap<_, _>>())
            .try_into()
            .unwrap_or_default();

          let body = if content_type == "application/json" {
            let mut content = String::new();
            request.as_reader().read_to_string(&mut content).unwrap();
            InvokeBody::Json(serde_json::from_str(&content).unwrap())
          } else {
            let mut content = Vec::new();
            request.as_reader().read_to_end(&mut content).unwrap();
            InvokeBody::Raw(content)
          };

          let invoke_request = InvokeRequest {
            cmd: cmd.to_string(),
            callback,
            error,
            url: Url::parse(&origin).expect("invalid IPC request URL"),
            body,
            headers,
            invoke_key,
          };

          let req_key = invoke_request.callback.0;
          requests.lock().unwrap().insert(req_key, request);

          let allowed_origins_ = allowed_origins.clone();
          let requests_ = requests.clone();
          window.on_message(
            invoke_request,
            Box::new(move |_webview, _cmd, response, callback, _error| {
              let request = requests_.lock().unwrap().remove(&callback.0).unwrap();
              let response = match response {
                InvokeResponse::Ok(r) => Ok(r),
                InvokeResponse::Err(e) => Err(e),
              };

              let tauri_response = if response.is_ok() { "ok" } else { "error" };

              let mut r = match response {
                Ok(tauri::ipc::InvokeResponseBody::Json(r)) => Response::from_string(r)
                  .with_header(Header {
                    field: "content-type".parse().unwrap(),
                    value: "application/json".parse().unwrap(),
                  }),
                Ok(tauri::ipc::InvokeResponseBody::Raw(r)) => Response::from_data(r.clone()),
                Err(tauri::ipc::InvokeError(e)) => {
                  Response::from_string(serde_json::to_string(&e).unwrap()).with_header(Header {
                    field: "content-type".parse().unwrap(),
                    value: "application/json".parse().unwrap(),
                  })
                }
              };

              r.add_header(Header {
                field: "Tauri-Response".parse().unwrap(),
                value: tauri_response.parse().unwrap(),
              });

              cors(&request, &mut r, &allowed_origins_);

              request.respond(r).unwrap();
            }),
          );
        } else {
          let mut r = Response::empty(404u16);
          cors(&request, &mut r, &allowed_origins);
          request.respond(r).unwrap();
        }
      }
    });
  }

  pub fn initialization_script(&self) -> String {
    include_str!("./invoke_system.js").replace("__PORT__", &self.port.to_string())
  }
}
