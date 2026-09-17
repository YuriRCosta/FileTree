use super::*;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

pub(super) struct SearchRun<'a> {
    pub(super) root: &'a Path,
    pub(super) spec: &'a SearchSpec,
    pub(super) show_hidden: bool,
    pub(super) limit: usize,
    pub(super) candidate_limit: usize,
    pub(super) started: Instant,
    pub(super) cancelled: &'a AtomicBool,
    pub(super) git_enabled: bool,
}

pub fn search_streaming(
    request: &SearchRequest<'_>,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(Value) -> AppResult<()>,
) -> Value {
    let root = match parse_path(request.root) {
        Ok(path) => path,
        Err(error) => return path_error(request.root, &error),
    };
    let needle = request.query.trim();
    let spec = match prepared_spec(&root, needle, request.options, cancelled) {
        Ok(spec) => spec,
        Err(payload) => return payload,
    };
    if spec.empty() && request.list.is_none() {
        return search_response(&root, "", "", 0, Settled::default(), Vec::new());
    }
    let limit = request.limit.clamp(1, SEARCH_CANDIDATE_CAP);
    let run = SearchRun {
        root: &root,
        spec: &spec,
        show_hidden: request.show_hidden,
        limit,
        candidate_limit: (limit * SEARCH_CANDIDATE_MULTIPLIER).min(SEARCH_CANDIDATE_CAP),
        started: Instant::now(),
        cancelled,
        git_enabled: request.git_enabled,
    };
    let pattern_text = spec.matcher_pattern();
    let case = index::case_matching(spec.options.case_sensitive);
    if let Some(file) = request.list {
        return list_search(&run, file, needle, &pattern_text, request.repository_roots);
    }
    if let Some(needle) = spec.content.clone() {
        return content_search(&run, &needle, &pattern_text, request.repository_roots);
    }
    if spec.scope != Scope::Here {
        return located_search(&run, needle, &pattern_text, request.repository_roots);
    }
    let shared = index::acquire(&root, request.show_hidden, request.fresh);
    let mut guard = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.set_pattern(&pattern_text, case);
    let mut settled = match settle(&run, &mut guard, needle, progress) {
        Ok(settled) => settled,
        Err(error) => return search_error(&root, needle, &spec.describe(), &error),
    };
    drop(guard);
    let mut rows = candidate_rows(&run, settled.hits.clone(), settled.ranked);
    let seen = decorate_rows(&run, &mut rows, request.repository_roots);
    let deleted_pattern = settled
        .ranked
        .then(|| index::parse_pattern(&pattern_text, case));
    append_deleted_matches(&mut rows, &run, &seen, deleted_pattern.as_ref());
    if cancelled.load(Ordering::Relaxed) {
        return search_error(&root, needle, &spec.describe(), &cancelled_error());
    }
    sort_rows(&mut rows, settled.ranked, &crate::frecency::scores());
    rows.truncate(limit);
    if request.tree {
        rows = tree_layout(&root, rows, request.git_enabled);
    }
    let filters = spec.describe();
    let overhead = serde_json::to_vec(&(path_text(&root), needle, &filters))
        .map(|data| data.len())
        .unwrap_or(SEARCH_RESPONSE_BYTES)
        .saturating_add(4096);
    retain_response_budget(&mut rows, overhead);
    settled.partial |= run.started.elapsed() >= SEARCH_DEADLINE;
    search_response(&root, needle, &filters, seen.roots.len(), settled, rows)
}

pub(super) fn content_search(
    run: &SearchRun<'_>,
    needle: &str,
    pattern_text: &str,
    repository_roots: &[String],
) -> Value {
    let spec = run.spec;
    let describe = spec.describe();
    let request = crate::grep::GrepRequest {
        root: run.root,
        needle,
        regex: spec.options.regex,
        case_sensitive: spec.options.case_sensitive,
        show_hidden: run.show_hidden,
        globs: &spec.globs(),
        timeout: SEARCH_DEADLINE.saturating_sub(run.started.elapsed()),
    };
    let mut files = match crate::grep::grep(&request, run.cancelled) {
        Ok(files) => files,
        Err(error) => return search_error(run.root, needle, &describe, &error),
    };
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    let case = index::case_matching(spec.options.case_sensitive);
    let pattern = (!pattern_text.is_empty()).then(|| index::parse_pattern(pattern_text, case));
    let mut matcher = Matcher::new(index::matcher_config());
    let mut rows = Vec::new();
    let mut hits = Vec::new();
    for file in files {
        let Ok(mut item) =
            entry_for_path_with_git(&run.root.join(&file.native), false, run.git_enabled)
        else {
            continue;
        };
        item["relative"] = json!(file.relative);
        let scored = match &pattern {
            Some(pattern) => index::score_text(pattern, &file.relative, &mut matcher),
            None => Some((0, Vec::new())),
        };
        let Some((_, indices)) = scored.filter(|_| spec_matches(spec, &item)) else {
            continue;
        };
        annotate_spans(spec, &mut item, &indices);
        item["hit_count"] = json!(file.hits.len());
        rows.push(item);
        hits.push(file.hits);
    }
    let seen = decorate_rows(run, &mut rows, repository_roots);
    let mut entries = Vec::new();
    let mut hit_count = 0_usize;
    for (item, file_hits) in rows.into_iter().zip(hits) {
        let (path, relative, mime) = (
            item["path"].as_str().unwrap_or_default().to_string(),
            item["relative"].as_str().unwrap_or_default().to_string(),
            item["mime"].clone(),
        );
        entries.push(item);
        for hit in file_hits {
            hit_count += 1;
            entries.push(json!({
                "name": hit["text"],
                "path": path,
                "relative": format!("{relative}:{}", hit["line"]),
                "line": hit["line"],
                "column": hit["column"],
                "is_dir": false,
                "is_symlink": false,
                "is_git_repo": false,
                "is_deleted": false,
                "size": -1,
                "size_text": "",
                "modified": "",
                "created": "",
                "kind": "Match",
                "mime": mime,
                "name_spans": hit["spans"],
                "relative_spans": ""
            }));
        }
        if entries.len() >= run.limit {
            break;
        }
    }
    entries.truncate(run.limit);
    let mut response = search_response(
        run.root,
        needle,
        &describe,
        seen.roots.len(),
        Settled::default(),
        entries,
    );
    response["content"] = json!(true);
    response["hits"] = json!(hit_count);
    response
}

pub fn lists_directory() -> PathBuf {
    crate::paths::state_dir().join("lists")
}

pub(super) fn list_search(
    run: &SearchRun<'_>,
    file: &str,
    needle: &str,
    pattern_text: &str,
    repository_roots: &[String],
) -> Value {
    let spec = run.spec;
    let describe = spec.describe();
    let list_path = match parse_path(file) {
        Ok(path) => path,
        Err(error) => return search_error(run.root, needle, &describe, &error),
    };
    let document = crate::secure::read_private_bounded(&list_path, 4 * 1024 * 1024)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(document) = document else {
        let error = io::Error::other("list file is missing or unreadable");
        return search_error(run.root, needle, &describe, &error);
    };
    let base = match parse_path(document["base"].as_str().unwrap_or("/")) {
        Ok(path) => path,
        Err(error) => return search_error(run.root, needle, &describe, &error),
    };
    let title = document["title"].as_str().unwrap_or("list").to_string();
    let case = index::case_matching(spec.options.case_sensitive);
    let pattern = (!pattern_text.is_empty()).then(|| index::parse_pattern(pattern_text, case));
    let mut matcher = Matcher::new(index::matcher_config());
    let mut hits = document["paths"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .take(10_000)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| {
            let path = parse_path(path).ok()?;
            let relative = relative_text(&path, &base);
            let relative = if relative == "." {
                display_path(&path)
            } else {
                relative
            };
            let (score, indices) = match &pattern {
                Some(pattern) => index::score_text(pattern, &relative, &mut matcher)?,
                None => (0, Vec::new()),
            };
            Some(Hit {
                entry: IndexEntry {
                    relative,
                    native: Some(path.into()),
                    is_dir: false,
                    is_symlink: false,
                },
                score,
                indices,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.entry.relative.cmp(&right.entry.relative))
    });
    let listed = SearchRun {
        root: &base,
        ..*run
    };
    let mut rows = candidate_rows(&listed, hits, pattern.is_some());
    let seen = decorate_rows(&listed, &mut rows, repository_roots);
    sort_rows(&mut rows, pattern.is_some(), &crate::frecency::scores());
    rows.truncate(run.limit);
    let mut response = search_response(
        &base,
        needle,
        &describe,
        seen.roots.len(),
        Settled {
            ranked: pattern.is_some(),
            partial: run.started.elapsed() >= SEARCH_DEADLINE,
            ..Settled::default()
        },
        rows,
    );
    response["list"] = json!(true);
    response["title"] = json!(title);
    response
}

pub(super) fn located_search(
    run: &SearchRun<'_>,
    needle: &str,
    pattern_text: &str,
    repository_roots: &[String],
) -> Value {
    let spec = run.spec;
    let describe = spec.describe();
    let Some(plocate) = crate::command::which("plocate") else {
        let error = io::Error::other("plocate is not installed");
        return search_error(run.root, needle, &describe, &error);
    };
    let Some(probe) = spec
        .terms
        .iter()
        .filter(|term| !term.negate && term.pattern.is_none())
        .max_by_key(|term| term.text.chars().count())
        .map(|term| term.text.clone())
    else {
        let error = io::Error::other("scope:everywhere needs one plain term");
        return search_error(run.root, needle, &describe, &error);
    };
    let home = expanded_path("~");
    let mut arguments = vec!["-0".to_string(), "-l".to_string(), "4000".to_string()];
    if !spec.options.case_sensitive {
        arguments.insert(0, "-i".to_string());
    }
    arguments.extend(["--".to_string(), probe]);
    let output = crate::command::CommandSpec::new(plocate)
        .args(arguments)
        .timeout(SEARCH_DEADLINE.saturating_sub(run.started.elapsed()))
        .limits(SEARCH_RESPONSE_BYTES, 64 * 1024)
        .run_cancellable(run.cancelled);
    let output = match output {
        Ok(output) if matches!(output.status.code(), Some(0 | 1)) => output,
        Ok(output) => {
            let error = io::Error::other(format!(
                "plocate exited with {}",
                output.status.code().unwrap_or(-1)
            ));
            return search_error(run.root, needle, &describe, &error);
        }
        Err(error) => return search_error(run.root, needle, &describe, &error),
    };
    let case = index::case_matching(spec.options.case_sensitive);
    let pattern = (!pattern_text.is_empty()).then(|| index::parse_pattern(pattern_text, case));
    let mut matcher = Matcher::new(index::matcher_config());
    let mut hits = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let path = Path::new(OsStr::from_bytes(line));
            if !path.is_absolute() || (spec.scope == Scope::Home && !path.starts_with(&home)) {
                return None;
            }
            let text = display_path(path);
            let (score, indices) = match &pattern {
                Some(pattern) => index::score_text(pattern, &text, &mut matcher)?,
                None => (0, Vec::new()),
            };
            Some(Hit {
                entry: IndexEntry {
                    relative: text,
                    native: Some(path.into()),
                    is_dir: false,
                    is_symlink: false,
                },
                score,
                indices,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.entry.relative.len().cmp(&right.entry.relative.len()))
    });
    let located = SearchRun {
        root: Path::new("/"),
        ..*run
    };
    let mut rows = candidate_rows(&located, hits, pattern.is_some());
    let seen = decorate_rows(&located, &mut rows, repository_roots);
    sort_rows(&mut rows, pattern.is_some(), &crate::frecency::scores());
    rows.truncate(run.limit);
    let mut response = search_response(
        run.root,
        needle,
        &describe,
        seen.roots.len(),
        Settled {
            ranked: pattern.is_some(),
            partial: output.stdout_truncated
                || output
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .count()
                    >= 4000
                || run.started.elapsed() >= SEARCH_DEADLINE,
            ..Settled::default()
        },
        rows,
    );
    response["scope"] = json!(format!("{:?}", spec.scope).to_lowercase());
    response
}

pub(super) fn tree_layout(root: &Path, mut rows: Vec<Value>, git_enabled: bool) -> Vec<Value> {
    rows.sort_by(|left, right| {
        left["relative"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["relative"].as_str().unwrap_or_default())
    });
    let mut emitted: HashSet<PathBuf> = HashSet::new();
    let mut layout = Vec::with_capacity(rows.len() * 2);
    for mut row in rows {
        let Ok(path) = parse_path(row["path"].as_str().unwrap_or_default()) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let parts = relative.components().collect::<Vec<_>>();
        for depth in 0..parts.len().saturating_sub(1) {
            let prefix = parts[..=depth].iter().collect::<PathBuf>();
            if !emitted.insert(root.join(&prefix)) {
                continue;
            }
            if let Ok(mut ancestor) =
                entry_for_path_with_git(&root.join(&prefix), false, git_enabled)
            {
                ancestor["relative"] = json!(display_path(&prefix));
                ancestor["depth"] = json!(depth);
                ancestor["expanded"] = json!(true);
                ancestor["ancestor"] = json!(true);
                layout.push(ancestor);
            }
        }
        row["depth"] = json!(parts.len().saturating_sub(1));
        emitted.insert(path);
        layout.push(row);
    }
    layout
}

pub(super) fn prepared_spec(
    root: &Path,
    needle: &str,
    options: SearchOptions,
    cancelled: &AtomicBool,
) -> Result<SearchSpec, Value> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(search_error(root, needle, "", &cancelled_error()));
    }
    if needle.len() > SEARCH_QUERY_BYTES {
        let error = io::Error::new(io::ErrorKind::InvalidInput, "Search query exceeds 64 KiB");
        return Err(search_error(root, needle, "", &error));
    }
    parse_search_query(needle, options).map_err(|message| {
        let error = io::Error::new(io::ErrorKind::InvalidInput, message);
        search_error(root, needle, "", &error)
    })
}

#[derive(Clone, Debug, Default)]
pub(super) struct Settled {
    pub(super) hits: Vec<Hit>,
    pub(super) walk: index::WalkStatus,
    pub(super) indexed: usize,
    pub(super) matched: usize,
    pub(super) partial: bool,
    pub(super) ranked: bool,
}

pub(super) fn settle(
    run: &SearchRun<'_>,
    guard: &mut index::PathIndex,
    needle: &str,
    progress: &mut dyn FnMut(Value) -> AppResult<()>,
) -> Result<Settled, io::Error> {
    let spec = run.spec;
    let mut accept = |relative: &str, flags: &IndexFlags| {
        spec.accepts_entry(relative, flags) && metadata_matches(run, relative, flags)
    };
    let mut last_emit = run.started;
    loop {
        let status = guard.tick(SEARCH_TICK_MS);
        let walk = guard.status();
        if run.cancelled.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        let deadline = run.started.elapsed() >= SEARCH_DEADLINE;
        let settled = !status.running && !walk.running && guard.indexed() >= walk.walked;
        if settled || deadline {
            let index_error = guard.error();
            if !index_error.is_empty() && guard.indexed() == 0 && !walk.running {
                return Err(io::Error::other(index_error));
            }
            return Ok(Settled {
                hits: guard.hits(run.candidate_limit, &mut accept),
                walk,
                indexed: guard.indexed(),
                matched: guard.matched(),
                partial: walk.running || deadline || run.started.elapsed() >= SEARCH_DEADLINE,
                ranked: guard.ranked(),
            });
        }
        if walk.running && last_emit.elapsed() >= SEARCH_PROGRESS_INTERVAL {
            let settled = Settled {
                hits: guard.hits(run.candidate_limit, &mut accept),
                walk,
                indexed: guard.indexed(),
                matched: guard.matched(),
                partial: true,
                ranked: guard.ranked(),
            };
            let rows = candidate_rows(run, settled.hits.clone(), settled.ranked);
            let payload = search_response(run.root, needle, "", 0, settled, rows);
            progress(payload).map_err(|_| cancelled_error())?;
            last_emit = Instant::now();
        }
        if !status.running && walk.running {
            thread::sleep(SEARCH_IDLE_SLEEP);
        }
    }
}

pub(super) fn search_response(
    root: &Path,
    needle: &str,
    filters: &str,
    git_repositories: usize,
    settled: Settled,
    rows: Vec<Value>,
) -> Value {
    json!({
        "ok": true,
        "root": path_text(root),
        "query": needle,
        "filters": filters,
        "backend": SEARCH_BACKEND,
        "git_repositories": git_repositories,
        "partial": settled.partial,
        "walked": settled.walk.walked,
        "indexed": settled.indexed,
        "matched": settled.matched,
        "truncated": settled.walk.truncated || settled.hits.len() > rows.len(),
        "ranked": settled.ranked,
        "entries": rows
    })
}

pub(super) fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "operation cancelled")
}

pub(super) fn search_error(
    root: &Path,
    query: &str,
    filters: &str,
    error: &(dyn std::error::Error + 'static),
) -> Value {
    let mut payload = error_payload(root, error);
    payload["query"] = json!(query);
    payload["filters"] = json!(filters);
    payload["backend"] = json!(SEARCH_BACKEND);
    payload["git_repositories"] = json!(0);
    payload["partial"] = json!(false);
    payload["walked"] = json!(0);
    payload["indexed"] = json!(0);
    payload["matched"] = json!(0);
    payload["truncated"] = json!(false);
    payload["ranked"] = json!(false);
    payload["entries"] = json!([]);
    payload
}
