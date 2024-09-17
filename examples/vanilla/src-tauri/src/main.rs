#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

#[tauri::command]
fn my_command(args: u64) -> Result<String, ()> {
  println!("executed command with args {args:?}");
  Ok("executed".into())
}

#[tauri::command]
fn my_command_raw(request: tauri::ipc::Request<'_>) -> Result<tauri::ipc::Response, ()> {
  println!("executed command with request {request:?}");
  Ok(tauri::ipc::Response::new(
    "executed raw".as_bytes().to_vec(),
  ))
}

fn main() {
  // Allow from all origins for testing purposes.
  // Should be allow listed to reduce risks of accidential exposure to other networks.
  let http = tauri_invoke_http::Invoke::new(["*"]);
  tauri::Builder::default()
    .invoke_system(http.initialization_script())
    .setup(move |app| {
      http.start(app.handle());
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![my_command, my_command_raw])
    .run(tauri::generate_context!())
    .expect("error while running tauri application")
}
