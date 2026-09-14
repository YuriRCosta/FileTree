use fileblade::index::{
    case_matching, literal_tier_path, matcher_config, parse_pattern, score_path_name,
};

#[test]
fn literal_matches_outrank_scattered_ones() {
    let needle = "video";
    let pattern = parse_pattern(needle, case_matching(false));
    let mut matcher = nucleo::Matcher::new(matcher_config());
    let mut rows: Vec<(u8, u32, &str)> = [
        "/src/vivid-life/openspec/changes/vivid-life-ontology",
        "/src/pbir-cli/tests/pythonVisual_Order_Lines_Order_Lines_(PY)",
        "/src/agents/vault/claude/skills/video",
        "/home/example/Videos",
        "/src/pbir-cli/tests/visual-data-roles.Report",
    ]
    .into_iter()
    .filter_map(|path| {
        score_path_name(&pattern, path, &mut matcher)
            .map(|(score, _)| (literal_tier_path(path, needle), score, path))
    })
    .collect();
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(right.2))
    });
    for (tier, score, path) in &rows {
        println!("tier {tier}  score {score:>4}  {path}");
    }
    assert_eq!(rows[0].2, "/src/agents/vault/claude/skills/video");
    assert_eq!(rows[0].0, 0, "an exact name is tier 0");
    assert_eq!(rows[1].2, "/home/example/Videos");
    assert_eq!(rows[1].0, 1, "a prefix match is tier 1");
    assert!(
        rows.iter().skip(2).all(|(tier, _, _)| *tier == 3),
        "everything below the literal hits is a scattered match"
    );
}
