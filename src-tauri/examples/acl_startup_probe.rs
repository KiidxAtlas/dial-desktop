use tauri::{WebviewUrl, WebviewWindowBuilder};
fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(20));
        std::process::exit(3)
    });
    tauri::Builder::default().setup(|app| {WebviewWindowBuilder::new(app,"main",WebviewUrl::default()).visible(false).on_document_title_changed(|_,t| {if t.starts_with("ACL_PROBE:"){println!("{t}");std::process::exit(if t=="ACL_PROBE:subscribed"{0}else{1})}}).on_page_load(|w,p|{if matches!(p.event(),tauri::webview::PageLoadEvent::Finished){w.eval(r#"(async()=>{const api=window.__TAURI_INTERNALS__; try{const id=await api.invoke('plugin:event|listen',{event:'snapshot',target:{kind:'Any'},handler:api.transformCallback(()=>{})});await api.invoke('plugin:event|unlisten',{event:'snapshot',eventId:id});document.title='ACL_PROBE:subscribed'}catch(e){document.title='ACL_PROBE:'+String(e)}})()"#).unwrap();}}).build()?; Ok(())}).run(tauri::generate_context!()).unwrap()
}
