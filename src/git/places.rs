use super::run_git;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const PLACE_TIMEOUT: Duration = Duration::from_secs(3);
const PLACE_BYTES: usize = 1024 * 1024;

fn git_lines<const N: usize>(
    root: &Path,
    arguments: [&str; N],
    cancelled: &AtomicBool,
) -> Option<Vec<String>> {
    let output = run_git(
        [OsString::from("-C"), root.as_os_str().to_owned()]
            .into_iter()
            .chain(arguments.into_iter().map(OsString::from)),
        PLACE_TIMEOUT,
        PLACE_BYTES,
        cancelled,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().map(ToOwned::to_owned).collect())
}

pub(super) fn remote_only_upstream(
    root: &Path,
    branch: &str,
    cancelled: &AtomicBool,
) -> Option<String> {
    let refs = git_lines(
        root,
        [
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
        cancelled,
    )?;
    if refs
        .iter()
        .any(|name| name == &format!("refs/heads/{branch}"))
    {
        return None;
    }
    let mut candidates = refs.iter().filter_map(|name| {
        let rest = name.strip_prefix("refs/remotes/")?;
        let (remote, short) = rest.split_once('/')?;
        (short == branch).then(|| format!("{remote}/{branch}"))
    });
    let upstream = candidates.next()?;
    candidates.next().is_none().then_some(upstream)
}
