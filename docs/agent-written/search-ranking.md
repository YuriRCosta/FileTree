# Search ranking

How Quick Nav and the recent-files list order their results.

## The problem with a bare fuzzy score

Both lists score candidates with nucleo, which matches a query as a
*subsequence*: the letters have to appear in order, but not together. That is
what makes fuzzy search feel good on long paths, and it is also why a query can
bury the answer.

Typing `video` subsequence-matches `vivid-life-ontology`, `visual-data-roles`
and `pythonVisual_Order_Lines_Order_Lines_(PY)`. Those score within about 30
points of `Videos`, so a handful of near-ties decided by visit frequency can
push a directory the user literally named down the list or off the end of it.

Case was never the issue. Both lists parse their pattern with
`CaseMatching::Ignore`, so `video` and `Videos` match identically.

## The tier

`index::literal_tier` grades how plainly the query appears in a candidate's
final path component, and the tier is compared **before** the nucleo score:

```yaml
0: the name equals the query        video      <- "video"
1: the name starts with the query   Videos     <- "video"
2: the name contains the query      my-videos  <- "video"
3: anything else                    vivid-life-ontology
```

Sort order is tier ascending, then nucleo score descending, then frequency or
frecency descending, then path. So a name the user could have typed outright
always sits above one that merely contains its letters in order, and within a
tier the old ordering is unchanged.

An empty query is tier 0 for every candidate, which leaves the plain
frequency-ordered listing alone. A query with spaces parses into several nucleo
atoms and will usually match no single literal substring, so it lands entirely
in tier 3 and behaves exactly as it did before.

`tests/quicknav_ranking.rs` pins the order for the `video` case.

## Where it applies

- `quicknav::quicknav_cancellable` - Quick Nav, over zoxide plus the directory
  index under the current root
- `frecency::list` - the recent list

`search::rows::sort_rows` still ranks on the nucleo score alone. It sorts
already-built JSON rows and is not handed the query, so tiering it means
threading the query through first.

## A related fix

`frecency::list` took its result limit *before* building rows, and row
construction drops entries whose path has since disappeared. A list of 20 could
therefore return 12. It now builds rows first and takes the limit from what
survived.
