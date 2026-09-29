//! Systemwide window commands via the XDG `GlobalShortcuts` portal. Portal work
//! stays off the Slint thread; only granted activations enter the event loop.
use crate::instance::Request;
use crate::shortcuts::Shortcuts;
use crate::windows;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str};

type PortalResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

static SENDER: OnceLock<mpsc::Sender<Shortcuts>> = OnceLock::new();
static STATUS: Mutex<String> = Mutex::new(String::new());
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(0);

pub fn status() -> String {
    STATUS.lock().unwrap().clone()
}

fn publish(message: impl Into<String>) {
    let message = message.into();
    STATUS.lock().unwrap().clone_from(&message);
    let _ = slint::invoke_from_event_loop(move || windows::set_global_shortcut_status(&message));
}

pub fn start() {
    let (tx, rx) = mpsc::channel();
    if SENDER.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = run(rx) {
            publish(format!(
                "Systemwide shortcuts unavailable: {error}. Use compositor bindings for toge-slint --toggle, --hide, or --new-window."
            ));
        }
    });
    refresh(&Shortcuts::load(&crate::shortcuts::path()));
}

pub fn refresh(shortcuts: &Shortcuts) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.send(shortcuts.clone());
    }
}

fn run(rx: mpsc::Receiver<Shortcuts>) -> PortalResult<()> {
    let connection = Connection::session()?;
    let portal = Proxy::new(&connection, DEST, PATH, INTERFACE)?;
    let _: u32 = portal.get_property("version")?;
    let mut activations = portal.receive_signal("Activated")?;
    let active = Arc::new(Mutex::new(None::<String>));
    let active_listener = active.clone();
    std::thread::spawn(move || {
        for message in &mut activations {
            let body: Result<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>), _> =
                message.body().deserialize();
            let Ok((session, id, _, _)) = body else {
                continue;
            };
            if active_listener.lock().unwrap().as_deref() != Some(session.as_str()) {
                continue;
            }
            let request = match id.as_str() {
                "toggle-window" => Request::Toggle,
                "show-window" => Request::Show,
                "hide-window" => Request::Hide,
                "global-new-window" => Request::NewWindow,
                _ => continue,
            };
            let _ = slint::invoke_from_event_loop(move || windows::handle(request));
        }
    });
    for shortcuts in rx {
        if let Some(previous) = active.lock().unwrap().take()
            && let Ok(session) = Proxy::new(
                &connection,
                DEST,
                previous.as_str(),
                "org.freedesktop.portal.Session",
            )
        {
            let _: Result<(), _> = session.call("Close", &());
        }
        let entries = shortcuts.global_entries();
        if entries.is_empty() {
            publish(
                "No systemwide shortcuts assigned. Add one above to request it from your desktop.",
            );
            continue;
        }
        publish("Requesting systemwide shortcuts from the desktop…");
        match bind(&connection, &portal, &entries) {
            Ok((session, granted)) => {
                *active.lock().unwrap() = Some(session.to_string());
                publish(format!(
                    "{granted} systemwide shortcut(s) granted by the desktop."
                ));
            }
            Err(error) => publish(format!("Could not register systemwide shortcuts: {error}")),
        }
    }
    Ok(())
}

fn bind(
    connection: &Connection,
    portal: &Proxy<'_>,
    entries: &[(&str, &str, String)],
) -> PortalResult<(OwnedObjectPath, usize)> {
    let create = request(connection, |options| portal.call("CreateSession", &options))?;
    let session_string: String = create
        .get("session_handle")
        .ok_or_else(|| io::Error::other("Portal did not return a session"))?
        .clone()
        .try_into()?;
    let session = OwnedObjectPath::try_from(session_string)?;
    let definitions: Vec<(String, HashMap<String, OwnedValue>)> = entries
        .iter()
        .map(|(id, label, trigger)| {
            let mut properties = HashMap::new();
            properties.insert(
                "description".to_string(),
                OwnedValue::from(Str::from(*label)),
            );
            properties.insert(
                "preferred_trigger".to_string(),
                OwnedValue::from(Str::from(trigger.as_str())),
            );
            ((*id).to_string(), properties)
        })
        .collect();
    let response = match request(connection, |options| {
        portal.call(
            "BindShortcuts",
            &(session.clone(), definitions, "", options),
        )
    }) {
        Ok(response) => response,
        Err(error) => {
            if let Ok(proxy) = Proxy::new(
                connection,
                DEST,
                session.as_str(),
                "org.freedesktop.portal.Session",
            ) {
                let _: Result<(), _> = proxy.call("Close", &());
            }
            return Err(error);
        }
    };
    let granted = response
        .get("shortcuts")
        .and_then(|value| {
            Vec::<(String, HashMap<String, OwnedValue>)>::try_from(value.clone()).ok()
        })
        .map_or(0, |shortcuts| shortcuts.len());
    Ok((session, granted))
}

fn request(
    connection: &Connection,
    call: impl FnOnce(HashMap<String, OwnedValue>) -> zbus::Result<OwnedObjectPath>,
) -> PortalResult<HashMap<String, OwnedValue>> {
    let unique = connection
        .unique_name()
        .ok_or_else(|| io::Error::other("No D-Bus sender name"))?;
    let sender = unique.as_str().trim_start_matches(':').replace('.', "_");
    let token = format!(
        "toge_{}_{}",
        std::process::id(),
        NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
    );
    let expected = format!("{PATH}/request/{sender}/{token}");
    let request = Proxy::new(
        connection,
        DEST,
        expected.as_str(),
        "org.freedesktop.portal.Request",
    )?;
    let mut responses = request.receive_signal("Response")?;
    let mut options = HashMap::new();
    options.insert(
        "handle_token".to_string(),
        OwnedValue::from(Str::from(token.as_str())),
    );
    options.insert(
        "session_handle_token".to_string(),
        OwnedValue::from(Str::from(format!("session_{token}").as_str())),
    );
    let handle = call(options)?;
    if handle.as_str() != expected {
        return Err(io::Error::other("Portal returned an unexpected request handle").into());
    }
    let message = responses
        .next()
        .ok_or_else(|| io::Error::other("Portal request closed"))?;
    let (response, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
    if response != 0 {
        return Err(io::Error::other(format!(
            "desktop declined shortcut request (code {response})"
        ))
        .into());
    }
    Ok(results)
}
