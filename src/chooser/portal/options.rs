use crate::chooser::{Filter, Mode, Offer, Request};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use zbus::zvariant::{OwnedValue, Value};

pub type Options = HashMap<String, OwnedValue>;

pub fn offer(
    handle: &str,
    caller: &str,
    parent_window: &str,
    title: &str,
    save: bool,
    mut options: Options,
) -> Result<Offer, String> {
    let accept_label = take::<String>(&mut options, "accept_label")?.unwrap_or_default();
    let modal = take::<bool>(&mut options, "modal")?.unwrap_or(true);
    let multiple = take::<bool>(&mut options, "multiple")?.unwrap_or(false);
    let directory = take::<bool>(&mut options, "directory")?.unwrap_or(false);
    let filters = take_filters(&mut options, "filters")?;
    let current_filter = take_filter(&mut options, "current_filter")?;
    let current_folder = take_path(&mut options, "current_folder")?;
    let current_name = take::<String>(&mut options, "current_name")?.unwrap_or_default();
    let current_file = take_path(&mut options, "current_file")?;
    let choices =
        take::<Vec<(String, String, Vec<(String, String)>, String)>>(&mut options, "choices")?;
    if choices.is_some_and(|choices| !choices.is_empty()) {
        return Err("Portal chooser choices are unsupported".into());
    }

    let (current_folder, current_name) = if save {
        if let Some(current_file) = current_file {
            if !current_file.is_absolute() {
                return Err("Portal current_file must be an absolute path".into());
            }
            let current_name = current_file
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "Portal current_file must have a UTF-8 filename".to_string())?
                .to_owned();
            (current_file.parent().map(PathBuf::from), current_name)
        } else {
            (current_folder, current_name)
        }
    } else {
        (current_folder, String::new())
    };
    let offer = Offer {
        handle: handle.into(),
        caller: caller.into(),
        parent_window: parent_window.into(),
        title: title.into(),
        accept_label,
        modal,
        current_folder,
        current_name,
        mode: if save {
            Mode::Save
        } else if directory {
            Mode::Folder
        } else {
            Mode::Open
        },
        multiple: multiple && !save,
        filters,
        current_filter,
    };
    Request::new(offer.clone()).map_err(|error| error.to_string())?;
    Ok(offer)
}

fn take<T>(options: &mut Options, name: &str) -> Result<Option<T>, String>
where
    T: TryFrom<Value<'static>>,
    T::Error: Into<zbus::zvariant::Error>,
{
    options
        .remove(name)
        .map(|value| {
            Value::from(value)
                .downcast::<T>()
                .map_err(|_| format!("Invalid portal option type: {name}"))
        })
        .transpose()
}

fn take_filters(options: &mut Options, name: &str) -> Result<Vec<Filter>, String> {
    take::<Vec<(String, Vec<(u32, String)>)>>(options, name)?.map_or_else(
        || Ok(Vec::new()),
        |filters| {
            Ok(filters
                .into_iter()
                .map(|(name, values)| Filter(name, values))
                .collect())
        },
    )
}

fn take_filter(options: &mut Options, name: &str) -> Result<Option<Filter>, String> {
    take::<(String, Vec<(u32, String)>)>(options, name)?
        .map_or_else(|| Ok(None), |(name, values)| Ok(Some(Filter(name, values))))
}

fn take_path(options: &mut Options, name: &str) -> Result<Option<PathBuf>, String> {
    take::<Vec<u8>>(options, name)?
        .map_or_else(|| Ok(None), |bytes| decode_path(name, bytes).map(Some))
}

fn decode_path(name: &str, mut bytes: Vec<u8>) -> Result<PathBuf, String> {
    if bytes.pop() != Some(0) || bytes.contains(&0) || bytes.is_empty() {
        return Err(format!(
            "Portal {name} must be a nonempty nul-terminated path"
        ));
    }
    let path = OsString::from_vec(bytes);
    if path.to_str().is_none() {
        return Err(format!("Portal {name} must be a UTF-8 path"));
    }
    Ok(PathBuf::from(path))
}
