.pragma library

var SORT_KEYS = ["name", "size", "type", "modified", "created", "repo", "branch", "worktree", "git", "git-modified", "git-deleted", "git-new"]
var FILTER_KEYS = ["size", "modified", "created"]
var GIT_RANK = { U: 7, D: 6, M: 5, T: 5, R: 4, C: 3, A: 2, "?": 1 }

function normalizeOne(raw) {
  var source = raw && typeof raw === "object" ? raw : ({})
  var key = String(source.key || "")
  if (SORT_KEYS.indexOf(key) < 0) return null
  return { key: key, desc: source.desc === true }
}

function normalizeSorts(raw) {
  var list = Array.isArray(raw) ? raw : (raw && typeof raw === "object" && raw.key !== undefined ? [raw] : [])
  var result = []
  var seen = {}
  for (var i = 0; i < list.length; i++) {
    var sort = normalizeOne(list[i])
    if (!sort || seen[sort.key]) continue
    seen[sort.key] = true
    result.push(sort)
  }
  return result
}

function normalizeFilter(raw) {
  var source = raw && typeof raw === "object" ? raw : ({})
  var result = ({})
  for (var i = 0; i < FILTER_KEYS.length; i++) {
    var clause = source[FILTER_KEYS[i]]
    if (!clause || typeof clause !== "object") continue
    var kept = ({})
    var fields = ["since", "until", "min", "max"]
    for (var j = 0; j < fields.length; j++)
      if (clause[fields[j]] !== undefined && clause[fields[j]] !== null && clause[fields[j]] !== "") kept[fields[j]] = clause[fields[j]]
    if (Object.keys(kept).length > 0) result[FILTER_KEYS[i]] = kept
  }
  return result
}

function gitRank(entry) {
  return GIT_RANK[String(entry.git_status || "")] || 0
}

var TEXT_FIELDS = { modified: "modified", created: "created", repo: "git_repo_name", branch: "git_branch", worktree: "git_worktree" }

function sortValue(entry, key) {
  if (key === "size") return Number(entry.size)
  if (key === "git") return gitRank(entry)
  var gitCounts = { "git-modified": "git_modified_count", "git-deleted": "git_deleted_count", "git-new": "git_new_count" }
  if (gitCounts[key]) return Number(entry[gitCounts[key]] || 0)
  if (key === "type") return String(entry.kind || "") + " " + String(entry.mime || "")
  if (TEXT_FIELDS[key]) return String(entry[TEXT_FIELDS[key]] || "")
  return String(entry.name || "").toLowerCase()
}

function compareBy(left, right, sort) {
  var a = sortValue(left, sort.key)
  var b = sortValue(right, sort.key)
  var result = typeof a === "number"
    ? a - b
    : String(a).localeCompare(String(b), undefined, { sensitivity: "base", numeric: true })
  return sort.desc ? -result : result
}

function compareEntries(left, right, sorts) {
  var dirs = (left.is_dir ? 0 : 1) - (right.is_dir ? 0 : 1)
  if (dirs !== 0) return dirs
  for (var i = 0; i < sorts.length; i++) {
    var result = compareBy(left, right, sorts[i])
    if (result !== 0) return result
  }
  return 0
}

function clauseMatches(entry, key, clause) {
  if (key === "size") {
    var size = Number(entry.size)
    if (!isFinite(size) || size < 0) return false
    return (clause.min === undefined || size >= Number(clause.min)) && (clause.max === undefined || size <= Number(clause.max))
  }
  var day = String(entry[key] || "").slice(0, 10)
  if (day === "") return false
  return (clause.since === undefined || day >= String(clause.since)) && (clause.until === undefined || day <= String(clause.until))
}

function passes(entry, filter) {
  if (entry.is_dir) return true
  for (var key in filter)
    if (!clauseMatches(entry, key, filter[key])) return false
  return true
}

function arrange(entries, sort, filter) {
  var list = Array.isArray(entries) ? entries : []
  var sorts = normalizeSorts(sort)
  var clauses = normalizeFilter(filter)
  var kept = Object.keys(clauses).length === 0 ? list.slice() : list.filter(function(entry) { return passes(entry, clauses) })
  if (sorts.length === 0) return kept
  var indexed = kept.map(function(entry, index) { return { entry: entry, index: index } })
  indexed.sort(function(left, right) { return compareEntries(left.entry, right.entry, sorts) || left.index - right.index })
  return indexed.map(function(item) { return item.entry })
}
