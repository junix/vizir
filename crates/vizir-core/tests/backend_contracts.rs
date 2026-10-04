use std::collections::{BTreeMap, BTreeSet};

use vizir_core::{
    BackendCapabilities, CapabilityDecision, CapabilityReport, CapabilityStatus, Color, Origin,
    Rect, ResolvedStyle, Scene2D, SceneNode, Transform2D, UnsupportedPolicy, negotiate_scene,
};

fn profile() -> BackendCapabilities {
    BackendCapabilities {
        backend: "contract-test".into(),
        version: "42.7".into(),
        accepted_ir: "scene2d".into(),
        supports: BTreeSet::from([
            "scene.2d".into(),
            "scene.2d.group".into(),
            "scene.2d.rect".into(),
        ]),
        unsupported: BTreeSet::new(),
        limits: BTreeMap::new(),
        lowering: BTreeMap::new(),
        unsupported_policy: UnsupportedPolicy::Error,
    }
}

fn origin() -> Origin {
    Origin {
        hir_node: "hir".into(),
        mir_node: "mir".into(),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: "contract-test".into(),
        explanation: "backend contract fixture".into(),
    }
}

fn rect(id: &str) -> SceneNode {
    SceneNode::Rect {
        id: id.into(),
        bounds: Rect::default(),
        origin: origin(),
        radius: 0.0,
        style: ResolvedStyle {
            fill: Color::hex("#000000"),
            stroke: Color::hex("#000000"),
            stroke_width: 0.0,
            opacity: 1.0,
        },
    }
}

fn group(id: &str, children: Vec<SceneNode>) -> SceneNode {
    SceneNode::Group {
        id: id.into(),
        bounds: Rect::default(),
        origin: origin(),
        transform: Transform2D::default(),
        opacity: 1.0,
        children,
    }
}

fn scene(nodes: Vec<SceneNode>) -> Scene2D {
    Scene2D {
        document_id: "doc".into(),
        width: 10.0,
        height: 10.0,
        background: Color::hex("#ffffff"),
        nodes,
        losses: Vec::new(),
    }
}

fn errors(report: &CapabilityReport) -> Vec<&CapabilityDecision> {
    report
        .decisions
        .iter()
        .filter(|decision| decision.status == CapabilityStatus::Error)
        .collect()
}

#[test]
fn existing_scene2d_aliases_accept_independent_backend_versions() {
    let scene = scene(vec![rect("rect")]);
    let mut profile = profile();
    for alias in ["scene2d", "scene2d-through-svg"] {
        profile.accepted_ir = alias.into();
        for version in ["1", "0.2", "42.7"] {
            profile.version = version.into();
            let report = negotiate_scene(&scene, &profile).unwrap();
            assert!(report.is_accepted(), "{alias}, backend version {version}");
            assert_eq!(report.backend_version, version);
            assert_eq!(report.accepted_ir, alias);
            // Successful identity/limit checks do not add synthetic decisions.
            assert_eq!(report.decisions.len(), 2);
        }
    }
}

#[test]
fn incompatible_ir_profiles_remain_inspectable_but_do_not_accept_scene2d() {
    let scene = scene(vec![rect("rect")]);
    for accepted_ir in [
        "scene3d",
        "vizmir",
        "scene2d@0.1",
        "scene2d@0.2",
        "scene2d-through-svg-extra",
        "SCENE2D",
        " scene2d",
    ] {
        let mut profile = profile();
        profile.accepted_ir = accepted_ir.into();
        profile.validate().unwrap();
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: BackendCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
        let report = negotiate_scene(&scene, &decoded).unwrap();
        assert!(
            !report.is_accepted(),
            "accepted incompatible IR {accepted_ir}"
        );
        let failures = errors(&report);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].source, "doc");
        assert_eq!(failures[0].feature, "accepted_ir");
        assert!(failures[0].reason.contains(accepted_ir));
        assert!(failures[0].reason.contains("Scene2D 0.1"));
        assert!(report.require_accepted().is_err());
    }
}

#[test]
fn wrong_ir_is_rejected_even_for_an_empty_scene() {
    let mut profile = profile();
    profile.accepted_ir = "scene3d".into();
    assert!(
        !negotiate_scene(&scene(vec![]), &profile)
            .unwrap()
            .is_accepted()
    );
}

#[test]
fn zero_node_limit_is_a_hard_zero_and_missing_limit_is_unbounded() {
    let mut profile = profile();
    let nonempty = scene(vec![rect("rect")]);
    assert!(negotiate_scene(&nonempty, &profile).unwrap().is_accepted());
    profile.limits.insert("max-nodes".into(), 0);
    assert!(
        negotiate_scene(&scene(vec![]), &profile)
            .unwrap()
            .is_accepted()
    );
    let report = negotiate_scene(&nonempty, &profile).unwrap();
    assert!(!report.is_accepted());
    let failures = errors(&report);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].feature, "limit.max-nodes");
    assert!(failures[0].reason.contains("1"));
    assert!(failures[0].reason.contains("0"));
    profile.limits.insert("max-nodes".into(), 1);
    assert!(negotiate_scene(&nonempty, &profile).unwrap().is_accepted());
    profile.limits.insert("max-nodes".into(), u64::MAX);
    assert!(negotiate_scene(&nonempty, &profile).unwrap().is_accepted());
}

#[test]
fn node_budget_counts_every_nested_node_including_empty_groups() {
    let scene = scene(vec![
        group("outer", vec![group("inner", vec![rect("leaf")])]),
        group("empty", vec![]),
        rect("sibling"),
    ]);
    let mut profile = profile();
    for limit in [0, 1, 3, 4] {
        profile.limits.insert("max-nodes".into(), limit);
        let report = negotiate_scene(&scene, &profile).unwrap();
        assert!(!report.is_accepted(), "incorrectly accepted limit {limit}");
        assert!(errors(&report)[0].reason.contains("5"));
    }
    profile.limits.insert("max-nodes".into(), 5);
    assert!(negotiate_scene(&scene, &profile).unwrap().is_accepted());
}

#[test]
fn node_budget_counts_occurrences_instead_of_distinct_ids_or_requirements() {
    let scene = scene(vec![rect("same"), rect("same")]);
    let mut profile = profile();
    profile.limits.insert("max-nodes".into(), 1);
    assert!(!negotiate_scene(&scene, &profile).unwrap().is_accepted());
}

#[test]
fn max_nodes_boundaries_32_and_33_are_inclusive() {
    let mut profile = profile();
    profile.limits.insert("max-nodes".into(), 32);
    for count in [0, 32, 33] {
        let scene = scene((0..count).map(|i| rect(&format!("rect-{i}"))).collect());
        let report = negotiate_scene(&scene, &profile).unwrap();
        assert_eq!(report.is_accepted(), count <= 32, "{count} nodes");
    }
}

#[test]
fn plain_groups_do_not_consume_clip_depth_at_zero_32_or_33() {
    let mut node = rect("leaf");
    for i in 0..33 {
        node = group(&format!("group-{i}"), vec![node]);
    }
    let scene = scene(vec![node]);
    let unlimited = negotiate_scene(&scene, &profile()).unwrap();
    for max_clip_depth in [0, 32, 33] {
        let mut profile = profile();
        profile
            .limits
            .insert("max-clip-depth".into(), max_clip_depth);
        let report = negotiate_scene(&scene, &profile).unwrap();
        assert_eq!(report, unlimited, "limit {max_clip_depth}");
    }
}

#[test]
fn unknown_or_misspelled_limit_keys_fail_closed_during_negotiation() {
    for key in ["max-text-bytes", "max_nodes", "Max-nodes", "max-nodes ", ""] {
        let mut profile = profile();
        profile.limits.insert(key.into(), 0);
        // Other dialect profiles may define different limits, so discovery
        // remains possible without claiming this negotiator understands them.
        profile.validate().unwrap();
        let report = negotiate_scene(&scene(vec![]), &profile).unwrap();
        assert!(!report.is_accepted(), "ignored limit {key:?}");
        let failures = errors(&report);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].feature, format!("limit.{key}"));
        assert!(failures[0].reason.contains("unsupported limit"));
    }
}

#[test]
fn every_contract_failure_is_reported_without_suppressing_missing_features() {
    let mut profile = profile();
    profile.accepted_ir = "scene3d".into();
    profile.limits = BTreeMap::from([("max-nodes".into(), 0), ("unknown-limit".into(), 10)]);
    profile.supports.remove("scene.2d.rect");
    let report = negotiate_scene(&scene(vec![rect("rect")]), &profile).unwrap();
    let failures = errors(&report);
    assert_eq!(failures.len(), 4);
    for feature in [
        "accepted_ir",
        "limit.max-nodes",
        "limit.unknown-limit",
        "scene.2d.rect",
    ] {
        assert!(failures.iter().any(|failure| failure.feature == feature));
    }
    let pairs = report
        .decisions
        .iter()
        .map(|d| (&d.source, &d.feature))
        .collect::<Vec<_>>();
    assert!(pairs.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn report_policy_does_not_authorize_unsupported_emission() {
    for policy in [UnsupportedPolicy::Error, UnsupportedPolicy::Report] {
        let mut profile = profile();
        profile.unsupported_policy = policy;
        profile.limits.insert("max-nodes".into(), 0);
        let report = negotiate_scene(&scene(vec![rect("rect")]), &profile).unwrap();
        assert!(!report.is_accepted());
        assert!(report.require_accepted().is_err());
        let serialized = serde_json::to_value(&report).unwrap();
        assert!(
            serialized["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["status"] == "error")
        );
    }
}

#[test]
fn repeated_limit_keys_are_rejected_instead_of_last_value_wins() {
    for limits in [
        r#"{"max-nodes":0,"max-nodes":100}"#,
        r#"{"max-nodes":100,"max-nodes":0}"#,
        r#"{"max-clip-depth":32,"max-clip-depth":32}"#,
        r#"{"future-limit":1,"future-limit":2}"#,
    ] {
        let json = format!(
            r#"{{"backend":"test","version":"1","accepted_ir":"scene2d","supports":["scene.2d"],"unsupported_policy":"error","limits":{limits}}}"#
        );
        let error = serde_json::from_str::<BackendCapabilities>(&json).unwrap_err();
        assert!(error.to_string().contains("duplicate limit"), "{error}");
    }
}

#[test]
fn canonical_u64_limits_round_trip_without_narrowing() {
    for limit in [0, 1, 32, (1u64 << 53) + 1, u64::MAX] {
        let mut profile = profile();
        profile.limits.insert("max-nodes".into(), limit);
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(
            serde_json::from_str::<BackendCapabilities>(&json).unwrap(),
            profile
        );
    }
}

#[test]
fn node_budget_includes_all_drawable_variants_but_not_svg_resources() {
    use vizir_core::{FontWeight, Point, TextAnchor, scene_capability_requirements};

    let style = ResolvedStyle {
        fill: Color::hex("#000000"),
        stroke: Color::hex("#000000"),
        stroke_width: 1.0,
        opacity: 1.0,
    };
    let bounds = Rect::default();
    let scene = scene(vec![group(
        "group",
        vec![
            rect("rect"),
            SceneNode::Circle {
                id: "circle".into(),
                bounds,
                origin: origin(),
                center: Point { x: 0.0, y: 0.0 },
                radius: 1.0,
                style: style.clone(),
            },
            SceneNode::Line {
                id: "line".into(),
                bounds,
                origin: origin(),
                from: Point { x: 0.0, y: 0.0 },
                to: Point { x: 1.0, y: 1.0 },
                style: style.clone(),
                marker_end: true,
            },
            SceneNode::Path {
                id: "path".into(),
                bounds,
                origin: origin(),
                commands: vec![],
                style,
                marker_end: true,
            },
            SceneNode::Text {
                id: "text".into(),
                bounds,
                origin: origin(),
                position: Point { x: 0.0, y: 0.0 },
                text: "text".into(),
                font_size: 1.0,
                anchor: TextAnchor::Start,
                color: Color::hex("#000000"),
                weight: FontWeight::Regular,
            },
        ],
    )]);
    let mut profile = profile();
    profile.supports = scene_capability_requirements(&scene)
        .into_iter()
        .map(|r| r.feature)
        .collect();
    profile.limits.insert("max-nodes".into(), 5);
    let report = negotiate_scene(&scene, &profile).unwrap();
    assert!(!report.is_accepted());
    assert!(errors(&report)[0].reason.contains("6"));
    profile.limits.insert("max-nodes".into(), 6);
    assert!(negotiate_scene(&scene, &profile).unwrap().is_accepted());
}
