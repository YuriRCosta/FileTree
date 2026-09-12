pub mod options;

use crate::{AppError, AppResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedValue};

const FRONTEND: &str = "org.freedesktop.portal.Desktop";
const BACKEND: &str = "org.freedesktop.impl.portal.desktop.fileblade";
type Response = (u32, HashMap<String, OwnedValue>);
type Requests = Arc<Mutex<HashMap<String, Arc<Pending>>>>;
type OfferInput<'a> = (ObjectPath<'a>, &'a str, &'a str, &'a str, options::Options);

struct Pending {
    owner: String,
    cancelled: AtomicBool,
    socket: Mutex<Option<UnixStream>>,
}

impl Pending {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

struct RequestObject(Arc<Pending>);

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl RequestObject {
    fn close(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        if header.sender().map(|sender| sender.as_str()) != Some(&self.0.owner) {
            return Err(zbus::fdo::Error::AccessDenied(
                "Request belongs to another sender".into(),
            ));
        }
        self.0.cancel();
        Ok(())
    }
}

struct Portal {
    requests: Requests,
    root: PathBuf,
    connection: Weak<zbus::Connection>,
}

impl Portal {
    async fn choose(
        &self,
        (handle, app_id, parent_window, title, options): OfferInput<'_>,
        save: bool,
        header: Header<'_>,
    ) -> zbus::fdo::Result<Response> {
        let connection = self
            .connection
            .upgrade()
            .ok_or_else(|| zbus::fdo::Error::Failed("Portal connection closed".into()))?;
        let owner = zbus::fdo::DBusProxy::new(&connection)
            .await?
            .get_name_owner(FRONTEND.try_into().unwrap())
            .await?;
        if header.sender().map(|sender| sender.as_str()) != Some(owner.as_str()) {
            return Err(zbus::fdo::Error::AccessDenied(
                "Only the desktop portal frontend may offer requests".into(),
            ));
        }
        if !handle
            .as_str()
            .starts_with("/org/freedesktop/portal/desktop/request/")
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Invalid portal request handle".into(),
            ));
        }
        let offer = options::offer(
            handle.as_str(),
            &format!("{owner}/{app_id}"),
            parent_window,
            title,
            save,
            options,
        )
        .map_err(zbus::fdo::Error::InvalidArgs)?;
        let pending = Arc::new(Pending {
            owner: owner.to_string(),
            cancelled: AtomicBool::new(false),
            socket: Mutex::new(None),
        });
        {
            let mut requests = self.requests.lock().unwrap();
            if requests.len() >= 16 || requests.contains_key(handle.as_str()) {
                return Err(zbus::fdo::Error::LimitsExceeded(
                    "Chooser request limit or duplicate handle".into(),
                ));
            }
            requests.insert(handle.to_string(), Arc::clone(&pending));
        }
        let registered = connection
            .object_server()
            .at(handle.clone(), RequestObject(Arc::clone(&pending)))
            .await;
        if !matches!(registered, Ok(true)) {
            self.requests.lock().unwrap().remove(handle.as_str());
            return Err(zbus::fdo::Error::Failed(
                "Cannot register chooser request".into(),
            ));
        }
        let current_owner = zbus::fdo::DBusProxy::new(&connection)
            .await?
            .get_name_owner(FRONTEND.try_into().unwrap())
            .await;
        if current_owner.as_ref().ok() != Some(&owner) {
            pending.cancel();
        }
        let root = self.root.clone();
        let worker = Arc::clone(&pending);
        let result = blocking::unblock(move || exchange(&root, &worker, offer)).await;
        let _ = connection
            .object_server()
            .remove::<RequestObject, _>(handle.clone())
            .await;
        self.requests.lock().unwrap().remove(handle.as_str());
        if pending.cancelled.load(Ordering::SeqCst) {
            return Ok((1, HashMap::new()));
        }
        result.map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl Portal {
    async fn open_file(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        options: options::Options,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<Response> {
        self.choose(
            (handle, app_id, parent_window, title, options),
            false,
            header,
        )
        .await
    }

    async fn save_file(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        options: options::Options,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<Response> {
        self.choose(
            (handle, app_id, parent_window, title, options),
            true,
            header,
        )
        .await
    }
}

fn exchange(root: &std::path::Path, pending: &Pending, offer: super::Offer) -> AppResult<Response> {
    let mut socket = crate::lease::transport::connect(root).map_err(|error| {
        AppError::command(format!(
            "Native chooser unavailable; start FileBlade and retry: {error}"
        ))
    })?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    {
        let mut owned = pending.socket.lock().unwrap();
        if pending.cancelled.load(Ordering::SeqCst) {
            return Ok((1, HashMap::new()));
        }
        *owned = Some(socket.try_clone()?);
    }
    socket.write_all(b"{\"v\":1,\"type\":\"hello\"}\n")?;
    let mut reader = BufReader::new(socket.try_clone()?);
    let mut hello = String::new();
    (&mut reader).take(4096).read_line(&mut hello)?;
    let hello: Value = serde_json::from_str(&hello)?;
    if hello["authority"] != true || hello["ok"] != true {
        return Err(AppError::command(
            "Native chooser authority handshake failed",
        ));
    }
    socket.set_read_timeout(Some(Duration::from_secs(900)))?;
    let frame = json!({"v":1,"type":"request","id":"portal","generation":1,
        "command":"chooser","arguments":["offer","--document",serde_json::to_string(&offer)?],
        "deadline_ms":900000});
    serde_json::to_writer(&mut socket, &frame)?;
    socket.write_all(b"\n")?;
    let mut response = String::new();
    (&mut reader).take(1024 * 1024).read_line(&mut response)?;
    let frame: Value = serde_json::from_str(&response)?;
    if frame["ok"] != true || frame["payload"]["ok"] != true {
        return Err(AppError::command(format!(
            "Native chooser failed: {}",
            frame["payload"]["error"]
                .as_str()
                .or(frame["error"].as_str())
                .unwrap_or("invalid response")
        )));
    }
    let outcome = &frame["payload"]["outcome"];
    if outcome["status"] == "cancelled" {
        return Ok((1, HashMap::new()));
    }
    if outcome["status"] != "accepted" {
        return Err(AppError::command("Invalid native chooser outcome"));
    }
    let uris: Vec<String> = serde_json::from_value(outcome["uris"].clone())?;
    let mut results = HashMap::new();
    results.insert(
        "uris".into(),
        OwnedValue::try_from(zbus::zvariant::Value::from(uris))
            .map_err(|error| AppError::command(error.to_string()))?,
    );
    if !outcome["filter"].is_null() {
        let filter: super::Filter = serde_json::from_value(outcome["filter"].clone())?;
        results.insert(
            "current_filter".into(),
            OwnedValue::try_from(zbus::zvariant::Value::from((filter.0, filter.1)))
                .map_err(|error| AppError::command(error.to_string()))?,
        );
    }
    Ok((0, results))
}

pub fn serve() -> AppResult<()> {
    let root = crate::lease::selected_root()?.unwrap_or_else(|| {
        crate::paths::xdg_home("XDG_STATE_HOME", "~/.local/state").join("omarchy/fileblade")
    });
    let authority = connect_authority(&root)?;
    let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
    let connection = zbus::blocking::Connection::session()
        .map_err(|error| AppError::command(error.to_string()))?;
    let asynchronous = Arc::new(connection.inner().clone());
    connection
        .object_server()
        .at(
            "/org/freedesktop/portal/desktop",
            Portal {
                requests: Arc::clone(&requests),
                root,
                connection: Arc::downgrade(&asynchronous),
            },
        )
        .map_err(|error| AppError::command(error.to_string()))?;
    let proxy = zbus::blocking::fdo::DBusProxy::new(&connection)
        .map_err(|error| AppError::command(error.to_string()))?;
    let changes = proxy
        .receive_name_owner_changed()
        .map_err(|error| AppError::command(error.to_string()))?;
    connection
        .request_name(BACKEND)
        .map_err(|error| AppError::command(error.to_string()))?;
    let authority_stop = authority.try_clone()?;
    let monitor = std::thread::spawn({
        let connection = connection.clone();
        move || monitor_authority(authority, connection)
    });
    let mut bus_error = None;
    for change in changes {
        let args = match change.args() {
            Ok(args) => args,
            Err(error) => {
                bus_error = Some(AppError::command(error.to_string()));
                break;
            }
        };
        if args.name().as_str() == FRONTEND {
            for request in requests.lock().unwrap().values() {
                if args
                    .old_owner()
                    .as_ref()
                    .is_some_and(|owner| owner.as_str() == request.owner)
                {
                    request.cancel();
                }
            }
        }
    }
    let _ = authority_stop.shutdown(Shutdown::Both);
    let _ = monitor.join();
    for request in requests.lock().unwrap().values() {
        request.cancel();
    }
    Err(bus_error.unwrap_or_else(|| AppError::command("Portal session bus disconnected")))
}

fn connect_authority(root: &Path) -> AppResult<UnixStream> {
    let mut stream = crate::lease::transport::connect(root).map_err(|error| {
        AppError::command(format!("Native portal authority unavailable: {error}"))
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"{\"v\":1,\"type\":\"hello\",\"view\":false}\n")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let count = (&mut reader).take(4096).read_line(&mut line)?;
    if count == 0 || !line.ends_with('\n') {
        return Err(AppError::command(
            "Native portal authority handshake failed",
        ));
    }
    let hello: Value = serde_json::from_str(&line)?;
    if hello.get("v").and_then(Value::as_u64) != Some(1)
        || hello.get("type").and_then(Value::as_str) != Some("hello")
        || hello.get("ok") != Some(&Value::Bool(true))
        || hello.get("authority") != Some(&Value::Bool(true))
    {
        return Err(AppError::command(
            "Native portal authority handshake failed",
        ));
    }
    let stream = reader.into_inner();
    stream.set_read_timeout(None)?;
    Ok(stream)
}

fn monitor_authority(mut stream: UnixStream, connection: zbus::blocking::Connection) {
    let mut buffer = [0; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => {
                let _ = connection.close();
                return;
            }
            Ok(_) => {}
        }
    }
}
