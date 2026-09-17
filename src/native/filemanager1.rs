use crate::{AppError, AppResult};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::fdo::{RequestNameFlags, RequestNameReply};

const NAME: &str = "org.freedesktop.FileManager1";
const IDLE_EXIT: Duration = Duration::from_secs(120);

struct FileManager {
    activity: Arc<Mutex<Instant>>,
}

impl FileManager {
    fn touch(&self) {
        *self.activity.lock().unwrap() = Instant::now();
    }

    async fn open(&self, uris: Vec<String>, properties: bool) -> zbus::fdo::Result<()> {
        self.touch();
        let result = blocking::unblock(move || super::open::run(&uris, properties)).await;
        self.touch();
        result.map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    async fn show_folders(&self, uris: Vec<String>, _startup_id: &str) -> zbus::fdo::Result<()> {
        self.open(uris, false).await
    }

    async fn show_items(&self, uris: Vec<String>, _startup_id: &str) -> zbus::fdo::Result<()> {
        self.open(uris, false).await
    }

    async fn show_item_properties(
        &self,
        uris: Vec<String>,
        _startup_id: &str,
    ) -> zbus::fdo::Result<()> {
        self.open(uris, true).await
    }
}

pub fn serve() -> AppResult<()> {
    let activity = Arc::new(Mutex::new(Instant::now()));
    let connection = zbus::blocking::Connection::session()
        .map_err(|error| AppError::command(error.to_string()))?;
    connection
        .object_server()
        .at(
            "/org/freedesktop/FileManager1",
            FileManager {
                activity: Arc::clone(&activity),
            },
        )
        .map_err(|error| AppError::command(error.to_string()))?;
    match connection.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner) => {}
        Ok(_) | Err(zbus::Error::NameTaken) => {
            return Err(AppError::command(format!(
                "{NAME} is owned by another application"
            )));
        }
        Err(error) => return Err(AppError::command(error.to_string())),
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if activity.lock().unwrap().elapsed() >= IDLE_EXIT {
            return Ok(());
        }
    }
}
