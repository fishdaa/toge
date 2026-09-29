mod access;
mod actions;
mod client;
mod format;
mod global_shortcuts;
mod instance;
mod model;
mod preferences;
mod shortcuts;
mod tray;
mod windows;
mod worker;
slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--request-watcher-access")
    {
        if args.len() != 2 {
            return Err("Usage: toge-slint --request-watcher-access path/to/toged".into());
        }
        return access::request(std::path::Path::new(&args[1]));
    }
    let request = match args.as_slice() {
        [] => instance::Request::from_arg(None),
        [arg] => instance::Request::from_arg(arg.to_str()),
        _ => None,
    }
    .ok_or("Usage: toge-slint [--new-window | --toggle | --hide]")?;
    let socket = instance::socket_path();
    match instance::send(&socket, request) {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(error) => eprintln!("Could not reach the running Toge instance: {error}"),
    }
    if request == instance::Request::Hide {
        // There is nothing to hide when no GUI instance is running.
        return Ok(());
    }
    // Without the socket this process still works, but later launches start
    // their own instance instead of opening or toggling a window here.
    let listener = instance::bind(&socket)
        .inspect_err(|error| eprintln!("Toge single-instance socket unavailable: {error}"))
        .ok();
    windows::open()?;
    let owns_socket = listener.is_some();
    if let Some(listener) = listener {
        instance::serve(listener, |request| {
            let _ = slint::invoke_from_event_loop(move || windows::handle(request));
        });
    }
    tray::start();
    global_shortcuts::start();
    // Hidden (toggled) windows keep the GUI running; closing the last window
    // quits unless the tray icon is registered.
    let result = slint::run_event_loop_until_quit();
    windows::shutdown();
    tray::stop();
    if owns_socket {
        let _ = std::fs::remove_file(&socket);
    }
    result?;
    Ok(())
}
