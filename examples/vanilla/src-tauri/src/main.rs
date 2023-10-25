#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

#[tauri::command]
fn my_command(args: u64) -> Result<String, ()> {
  println!("executed command with args {:?}", args);
  Ok("executed".into())
}

fn main() {
  /// Allow from all origins for testing purposes. 
  /// Should be allow listed to reduce risks of accidential exposure to other networks.
  let http = tauri_invoke_http::Invoke::new(["*"]});
  tauri::Builder::default()
    .invoke_system(http.initialization_script(), http.responder())
    .setup(move |app| {
      http.start(app.handle());
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![my_command])
    .run(tauri::generate_context!())
    .expect("error while running tauri application")
}
