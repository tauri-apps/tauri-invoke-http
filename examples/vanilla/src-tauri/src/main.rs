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
  let http = tauri_invoke_http::Invoke::new(if cfg!(feature = "custom-protocol") {
    if cfg!(windows) { ["https://tauri.localhost"] } else { ["tauri://localhost"] }
  } else {
    ["http://127.0.0.1:1430"]
  });
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
