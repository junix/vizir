use super::*;
use crate::{Color, FontWeight, Origin, Point, Rect, ResolvedStyle, TextAnchor, Transform2D};

fn minimal_scene() -> Scene2D {
    Scene2D {
        document_id: "doc".to_owned(),
        width: 10.0,
        height: 10.0,
        background: Color::transparent(),
        nodes: vec![SceneNode::Rect {
            id: "rect".to_owned(),
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            origin: Origin {
                hir_node: "rect".to_owned(),
                mir_node: "rect".to_owned(),
                data_key: None,
                data_lineage: Vec::new(),
                generated_by: "test".to_owned(),
                explanation: "test".to_owned(),
            },
            radius: 0.0,
            style: ResolvedStyle {
                fill: Color::hex("#000000"),
                stroke: Color::transparent(),
                stroke_width: 0.0,
                opacity: 1.0,
            },
        }],
        losses: Vec::new(),
    }
}

#[test]
fn missing_capabilities_are_explicit_errors() {
    let scene = minimal_scene();
    let capabilities = BackendCapabilities {
        backend: "empty".to_owned(),
        version: "1".to_owned(),
        accepted_ir: "scene2d".to_owned(),
        supports: BTreeSet::new(),
        unsupported: BTreeSet::new(),
        limits: BTreeMap::new(),
        lowering: BTreeMap::new(),
        unsupported_policy: UnsupportedPolicy::Error,
    };
    let report = negotiate_scene(&scene, &capabilities).unwrap();
    assert!(!report.is_accepted());
    assert!(
        report
            .decisions
            .iter()
            .any(|decision| decision.feature == "scene.2d.rect"
                && decision.status == CapabilityStatus::Error)
    );
}

#[test]
fn contradictory_capability_profile_is_rejected() {
    let capabilities = BackendCapabilities {
        backend: "broken".to_owned(),
        version: "1".to_owned(),
        accepted_ir: "scene2d".to_owned(),
        supports: BTreeSet::from(["scene.2d".to_owned()]),
        unsupported: BTreeSet::from(["scene.2d".to_owned()]),
        limits: BTreeMap::new(),
        lowering: BTreeMap::new(),
        unsupported_policy: UnsupportedPolicy::Error,
    };
    assert!(capabilities.validate().is_err());
}

fn backend_with(supports: &[&str]) -> BackendCapabilities {
    BackendCapabilities {
        backend: "test-backend".to_owned(),
        version: "1".to_owned(),
        accepted_ir: "scene2d".to_owned(),
        supports: supports.iter().map(|feature| feature.to_string()).collect(),
        unsupported: BTreeSet::new(),
        limits: BTreeMap::new(),
        lowering: BTreeMap::new(),
        unsupported_policy: UnsupportedPolicy::Error,
    }
}

#[test]
fn validate_rejects_blank_identity_and_undeclared_lowering_features() {
    let blank_identity = "VIZ-CAP-0003: backend, version, and accepted_ir must be non-empty";
    for blank in [
        (
            "backend",
            (|capabilities: &mut BackendCapabilities| {
                capabilities.backend = String::new();
            }) as fn(&mut BackendCapabilities),
        ),
        ("version", |capabilities: &mut BackendCapabilities| {
            capabilities.version = String::new();
        }),
        ("accepted_ir", |capabilities: &mut BackendCapabilities| {
            capabilities.accepted_ir = String::new();
        }),
    ] {
        let (field, blank_it) = blank;
        let mut capabilities = backend_with(&["scene.2d"]);
        blank_it(&mut capabilities);
        assert_eq!(
            capabilities.validate().unwrap_err().to_string(),
            blank_identity,
            "{field}"
        );
    }

    let mut undeclared = backend_with(&["scene.2d"]);
    undeclared
        .lowering
        .insert("paint.alpha".to_owned(), CapabilityStatus::Rasterized);
    assert_eq!(
        undeclared.validate().unwrap_err().to_string(),
        "VIZ-CAP-0005: lowering strategy references undeclared feature \"paint.alpha\""
    );
}

#[test]
fn negotiation_reports_declared_lowering_and_support_fidelity() {
    let scene = minimal_scene();
    let mut capabilities = backend_with(&["scene.2d", "paint.alpha", "scene.2d.rect"]);
    capabilities
        .lowering
        .insert("scene.2d.rect".to_owned(), CapabilityStatus::Rasterized);
    let report = negotiate_scene(&scene, &capabilities)
        .expect("declared features should negotiate without errors");

    assert_eq!(report.backend, "test-backend");
    assert_eq!(report.backend_version, "1");
    assert_eq!(report.document_id, "doc");
    assert_eq!(report.accepted_ir, "scene2d");
    // Decisions stay in deterministic (source, feature) order; an explicit
    // lowering strategy replaces the default Exact status and its reason.
    assert_eq!(
        report.decisions,
        vec![
            CapabilityDecision {
                source: "doc".to_owned(),
                feature: "paint.alpha".to_owned(),
                status: CapabilityStatus::Exact,
                reason: "backend declares exact support".to_owned(),
            },
            CapabilityDecision {
                source: "doc".to_owned(),
                feature: "scene.2d".to_owned(),
                status: CapabilityStatus::Exact,
                reason: "backend declares exact support".to_owned(),
            },
            CapabilityDecision {
                source: "rect".to_owned(),
                feature: "paint.alpha".to_owned(),
                status: CapabilityStatus::Exact,
                reason: "backend declares exact support".to_owned(),
            },
            CapabilityDecision {
                source: "rect".to_owned(),
                feature: "scene.2d.rect".to_owned(),
                status: CapabilityStatus::Rasterized,
                reason: "backend declares an explicit lowering strategy".to_owned(),
            },
        ]
    );
    assert!(report.is_accepted());
    assert!(report.require_accepted().is_ok());
}

#[test]
fn require_accepted_names_every_unmet_requirement_in_order() {
    let scene = minimal_scene();
    let mut capabilities = backend_with(&[]);
    capabilities.backend = "empty".to_owned();
    let report = negotiate_scene(&scene, &capabilities)
        .expect("negotiation reports errors instead of failing");
    assert!(!report.is_accepted());
    assert_eq!(
        report.require_accepted().unwrap_err().to_string(),
        "VIZ-CAP-0002: backend \"empty\" cannot lower: \
             doc requires paint.alpha, doc requires scene.2d, \
             rect requires paint.alpha, rect requires scene.2d.rect"
    );
}

#[test]
fn scene_requirements_collect_alpha_transform_and_marker_features() {
    let opaque = ResolvedStyle {
        fill: Color::hex("#112233"),
        stroke: Color::hex("#000000"),
        stroke_width: 0.0,
        opacity: 1.0,
    };
    let origin = Origin {
        hir_node: "node".to_owned(),
        mir_node: "node".to_owned(),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: "test".to_owned(),
        explanation: "test".to_owned(),
    };
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    // An opaque 6-digit background must not request paint.alpha, while
    // group opacity, a 9-digit text color, a transform, and a marker-end
    // each add their own requirement, deduplicated and sorted by
    // (source, feature).
    let scene = Scene2D {
        document_id: "doc".to_owned(),
        width: 10.0,
        height: 10.0,
        background: Color::hex("#112233"),
        nodes: vec![SceneNode::Group {
            id: "g".to_owned(),
            bounds,
            origin: origin.clone(),
            transform: Transform2D {
                translate: Point { x: 1.0, y: 0.0 },
                rotate_degrees: 0.0,
                scale: Point { x: 1.0, y: 1.0 },
            },
            opacity: 0.5,
            children: vec![
                SceneNode::Line {
                    id: "l".to_owned(),
                    bounds,
                    origin: origin.clone(),
                    from: Point { x: 0.0, y: 0.0 },
                    to: Point { x: 1.0, y: 1.0 },
                    style: opaque,
                    marker_end: true,
                },
                SceneNode::Text {
                    id: "t".to_owned(),
                    bounds,
                    origin,
                    position: Point { x: 0.0, y: 0.0 },
                    text: "t".to_owned(),
                    font_size: 1.0,
                    anchor: TextAnchor::default(),
                    color: Color::hex("#11223344"),
                    weight: FontWeight::default(),
                },
            ],
        }],
        losses: Vec::new(),
    };

    let requirement = |source: &str, feature: &str| CapabilityRequirement {
        source: source.to_owned(),
        feature: feature.to_owned(),
    };
    assert_eq!(
        scene_capability_requirements(&scene),
        vec![
            requirement("doc", "scene.2d"),
            requirement("g", "paint.alpha"),
            requirement("g", "scene.2d.group"),
            requirement("g", "scene.2d.transform"),
            requirement("l", "paint.marker-end"),
            requirement("l", "scene.2d.line"),
            requirement("t", "paint.alpha"),
            requirement("t", "scene.2d.text"),
        ]
    );
}
