// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{io::Read, net::SocketAddr};

use anyhow::Context;
use http_body_util::{BodyExt, Full};
use hyper::{
  body::{Buf, Bytes},
  header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_EXPOSE_HEADERS, CONTENT_TYPE, ORIGIN,
  },
  http::{header::HeaderValue, response::Builder as ResponseBuilder},
  service::service_fn,
  HeaderMap, Method, Request, Response, StatusCode,
};
use tauri::{
  http::HeaderName,
  ipc::{CallbackFn, InvokeBody, InvokeResponse},
  webview::InvokeRequest,
  AppHandle, Manager, Runtime, Url,
};
use tokio::net::TcpListener;

mod tokio_rt;

trait Cors {
  fn cors(self, request_headers: &HeaderMap, allowed_origins: &[String]) -> Self;
}

impl Cors for ResponseBuilder {
  fn cors(mut self, request_headers: &HeaderMap, allowed_origins: &[String]) -> Self {
    if allowed_origins.iter().any(|s| s == "*") {
      self = self.header(
        ACCESS_CONTROL_ALLOW_ORIGIN,
        "*".parse::<HeaderValue>().unwrap(),
      );
    } else if let Some((_, origin)) = request_headers
      .iter()
      .find(|(name, _value)| name.as_str() == "Origin")
    {
      if allowed_origins
        .iter()
        .any(|o| o.as_bytes() == origin.as_bytes())
      {
        self = self.header(ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
      }
    }

    self = self.header(
      ACCESS_CONTROL_EXPOSE_HEADERS,
      "Tauri-Response".parse::<HeaderValue>().unwrap(),
    );
    self = self.header(
      ACCESS_CONTROL_ALLOW_HEADERS,
      "*".parse::<HeaderValue>().unwrap(),
    );
    self = self.header(
      ACCESS_CONTROL_ALLOW_METHODS,
      "POST, OPTIONS".parse::<HeaderValue>().unwrap(),
    );
    self
  }
}

pub struct Invoke {
  allowed_origins: Vec<String>,
  port: u16,
}

impl Invoke {
  pub fn new<I: Into<String>, O: IntoIterator<Item = I>>(allowed_origins: O) -> Self {
    let port = portpicker::pick_unused_port().expect("failed to get unused port for invoke");
    Self {
      allowed_origins: allowed_origins.into_iter().map(|o| o.into()).collect(),
      port,
    }
  }

  pub fn start<R: Runtime>(&self, app: &AppHandle<R>) {
    let app = app.clone();
    let allowed_origins = self.allowed_origins.clone();
    let addr: SocketAddr = format!("127.0.0.1:{}", self.port).parse().unwrap();

    tauri::async_runtime::spawn(async move {
      let listener = TcpListener::bind(addr).await.unwrap();

      loop {
        let (stream, _) = listener.accept().await.unwrap();
        let io = tokio_rt::TokioIo::new(stream);

        let app = app.clone();
        let allowed_origins = allowed_origins.clone();

        tokio::task::spawn(async move {
          let app = app.clone();
          let allowed_origins = allowed_origins.clone();
          if let Err(err) = hyper::server::conn::http1::Builder::new()
            .serve_connection(
              io,
              service_fn(move |req| {
                let app = app.clone();
                let allowed_origins = allowed_origins.clone();

                async move {
                  let response = server_handler(&app, req, &allowed_origins).await;
                  hyper::Result::Ok(response)
                }
              }),
            )
            .await
          {
            log::error!("Failed to serve connection: {err:?}");
          }
        });
      }
    });
  }

  pub fn initialization_script(&self) -> String {
    include_str!("./invoke_system.js").replace("__PORT__", &self.port.to_string())
  }
}

async fn server_handler<R: Runtime>(
  app: &AppHandle<R>,
  req: Request<hyper::body::Incoming>,
  allowed_origins: &[String],
) -> Response<Full<Bytes>> {
  match *req.method() {
    Method::OPTIONS => Response::builder()
      .cors(req.headers(), allowed_origins)
      .body(Bytes::new().into())
      .unwrap(),
    Method::POST => {
      let (tx, rx) = tokio::sync::oneshot::channel();
      if let Err(e) = handle_request(app, req, tx, allowed_origins).await {
        Response::builder()
          .status(StatusCode::BAD_REQUEST)
          .body(format!("failed to process request: {e}").into())
          .unwrap()
      } else {
        rx.await.unwrap_or_else(|_| {
          Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body("failed to process request".into())
            .unwrap()
        })
      }
    }
    _ => Response::builder()
      .status(StatusCode::NOT_FOUND)
      .body("not found".into())
      .unwrap(),
  }
}

async fn handle_request<R: Runtime>(
  app: &AppHandle<R>,
  request: Request<hyper::body::Incoming>,
  tx: tokio::sync::oneshot::Sender<Response<Full<Bytes>>>,
  allowed_origins: &[String],
) -> anyhow::Result<()> {
  let url = request.uri().to_string();
  let pieces = url.split('/').collect::<Vec<_>>();
  let window_label = pieces[1];
  let cmd = pieces[2];

  if let Some(window) = app.get_webview_window(window_label) {
    let content_type = request
      .headers()
      .iter()
      .find(|(name, _value)| name == &CONTENT_TYPE)
      .map(|(_name, value)| value.to_str().unwrap_or_default())
      .unwrap_or_else(|| "application/json")
      .to_string();

    let origin = request
      .headers()
      .iter()
      .find(|(name, _value)| name == &ORIGIN)
      .map(|(_name, value)| value.to_str().unwrap_or_default().to_string())
      .context("invalid IPC request: no Origin header")?;
    let invoke_key = request
      .headers()
      .iter()
      .find(|(name, _value)| *name == "Tauri-Invoke-Key")
      .map(|(_name, value)| value.to_str().unwrap_or_default().to_string())
      .context("invalid IPC request: no Tauri-Invoke-Key header")?;
    let callback = CallbackFn(
      request
        .headers()
        .iter()
        .find(|(name, _value)| *name == "Tauri-Callback")
        .map(|(_name, value)| value.to_str().unwrap_or_default())
        .context("invalid IPC request: no Tauri-Callback header")?
        .parse()
        .context("invalid Tauri-Callback header")?,
    );
    let error = CallbackFn(
      request
        .headers()
        .iter()
        .find(|(name, _value)| *name == "Tauri-Error")
        .map(|(_name, value)| value.to_str().unwrap_or_default())
        .context("invalid IPC request: no Tauri-Error header")?
        .parse()
        .context("invalid Tauri-Error header")?,
    );

    let headers = request.headers().clone();

    let body = request.collect().await?.aggregate();

    let body = if content_type == "application/json" {
      InvokeBody::Json(serde_json::from_reader(body.reader())?)
    } else {
      let mut content = Vec::new();
      body.reader().read_to_end(&mut content)?;
      InvokeBody::Raw(content)
    };

    let headers_ = headers.clone();

    let invoke_request = InvokeRequest {
      cmd: cmd.to_string(),
      callback,
      error,
      url: Url::parse(&origin).expect("invalid IPC request URL"),
      body,
      headers,
      invoke_key,
    };

    let allowed_origins_ = allowed_origins.to_vec();
    window.on_message(
      invoke_request,
      Box::new(move |_webview, _cmd, response, _callback, _error| {
        let invoke_response = match response {
          InvokeResponse::Ok(r) => Ok(r),
          InvokeResponse::Err(e) => Err(e),
        };

        let tauri_response = if invoke_response.is_ok() {
          "ok"
        } else {
          "error"
        };

        let mut r = match invoke_response {
          Ok(tauri::ipc::InvokeResponseBody::Json(r)) => Response::builder()
            .cors(&headers_, &allowed_origins_)
            .header(
              CONTENT_TYPE,
              "application/json".parse::<HeaderValue>().unwrap(),
            )
            .body(Full::new(Bytes::from(r)))
            .unwrap(),
          Ok(tauri::ipc::InvokeResponseBody::Raw(r)) => Response::builder()
            .cors(&headers_, &allowed_origins_)
            .body(Full::new(Bytes::from(r)))
            .unwrap(),
          Err(tauri::ipc::InvokeError(e)) => Response::builder()
            .cors(&headers_, &allowed_origins_)
            .header(
              CONTENT_TYPE,
              "application/json".parse::<HeaderValue>().unwrap(),
            )
            .body(Full::new(Bytes::from(serde_json::to_string(&e).unwrap())))
            .unwrap(),
        };

        r.headers_mut().insert(
          "Tauri-Response".parse::<HeaderName>().unwrap(),
          tauri_response.parse().unwrap(),
        );

        tx.send(r).unwrap();
      }),
    );

    Ok(())
  } else {
    Err(anyhow::anyhow!("unknown window"))
  }
}
