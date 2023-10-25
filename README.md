# Tauri Invoke HTTP

This is a crate that provides a custom invoke system for Tauri using a localhost server.
Each message is delivered through a `XMLHttpRequest` and the server is responsible for replying to it.

## Usage

First, add the dependency to your `src-tauri/Cargo.toml` file:

```
[dependencies]
tauri-invoke-http = { git = "https://github.com/tauri-apps/tauri-invoke-http", branch = "dev" }
```

Then, setup the HTTP invoke system on the `main.rs` file:

```rust
fn main() {
  // initialize the custom invoke system as a HTTP server, allowing the given origins to access it.
  let http = tauri_invoke_http::Invoke::new(if cfg!(feature = "custom-protocol") {
    ["tauri://localhost"]
  } else {
    ["http://localhost:8080"]
  });
  tauri::Builder::default()
    .invoke_system(http.initialization_script(), http.responder())
    .setup(move |app| {
      http.start(app.handle());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application")
}
```

To invoke a custom command from your own or remote system you can use `curl` or similar tooling.
See [`examples/vanilla`](examples/vanilla/) to test this on your system.

An example command to invoke the `exit` command in the example Tauri app exposing port `18436` (randomly chosen port) could look like:

```sh
curl localhost:18436/main -H 'Content-Type: application/json' -d '{ "__tauriModule": "Process", "cmd": "exit", "callback": 1234, "error": 1234, "message": {"cmd": "exit", "exitCode": 1  } }'
```


