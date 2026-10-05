#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    build_compiled_scene, build_scene, build_themed_scene, compile, compile_with_context,
    compile_with_theme, lower_to_mir, parse_compiled_mir_json, parse_themed_mir_json,
    rematerialize_compiled_mir, rematerialize_mir,
};
use vizir_core::{
    Composition, Document, MirDataOperator, MirScale, MirView, NumericOutOfDomain, SceneNode,
    VizMir, compose_versioned, find_scene_node,
};
fn source(kind: &str, explicit: bool) -> Document {
    let mut v = json!({"kind":format!("chart.{kind}"),"id":"chart","frame":{"x":0,"y":0,"width":900,"height":550},"dataset":"d"});
    if kind == "bar" {
        v["category"] = json!({"field":"id"});
        v["value"] = json!({"field":"y"});
        if explicit {
            v["value"]["domain"] = json!([-1.0, 10.0]);
        }
    } else {
        v["x"] = json!({"field":"x"});
        v["y"] = json!({"field":"y"});
        if explicit {
            v["x"]["domain"] = json!([-0.0, 2.0]);
            v["y"]["domain"] = json!([-1.0, 10.0]);
        }
    }
    if kind == "area" {
        v["baseline"] = json!(0.0);
        v["order"] = json!("x-ascending");
    }
    serde_json::from_value(json!({"version":"0.7","id":"numeric","width":900,"height":550,"datasets":{"d":{"key":"id","rows":[{"id":"a","x":0.0,"y":2.0},{"id":"b","x":1.0,"y":8.0}]}},"views":[v]})).unwrap()
}
fn chart(m: &VizMir) -> &vizir_core::MirChart {
    let MirView::Chart(c) = &m.views[0] else {
        panic!()
    };
    c
}
fn set_row(m: &mut VizMir, field: &str, value: Value) {
    let MirDataOperator::Inline { rows } = &mut m.data.get_mut("data/d").unwrap().operator;
    rows[0].insert(field.into(), value);
}
#[test]
fn exact_domains_survive_all_chart_kinds_themes_measured_context_and_replay() {
    let (ctx, resources) = fixtures::profile();
    for kind in ["scatter", "line", "area", "bar"] {
        let d = source(kind, true);
        let c = compile(&d).unwrap();
        assert_eq!(c.mir.version, "0.7");
        for s in &chart(&c.mir).scales {
            if let MirScale::Linear {
                id,
                domain,
                out_of_domain,
                ..
            } = s
            {
                let expected = if id.ends_with("/x") {
                    [-0.0, 2.0]
                } else {
                    [-1.0, 10.0]
                };
                assert_eq!(domain.map(f64::to_bits), expected.map(f64::to_bits));
                assert_eq!(*out_of_domain, Some(NumericOutOfDomain::Reject));
            }
        }
        assert_eq!(build_scene(&c.mir).unwrap(), c.scene);
        assert_eq!(rematerialize_mir(&c.mir).unwrap(), c.mir);
        let measured = compile_with_context(&d, &ctx, &resources).unwrap();
        let restored =
            parse_compiled_mir_json(&serde_json::to_vec(&measured.mir).unwrap()).unwrap();
        assert_eq!(
            build_compiled_scene(&restored, &resources).unwrap(),
            measured.scene
        );
        assert_eq!(
            rematerialize_compiled_mir(&restored, &resources).unwrap(),
            restored
        );
        let themed = compile_with_theme(&d, "azure").unwrap();
        let restored = parse_themed_mir_json(&serde_json::to_vec(&themed.mir).unwrap()).unwrap();
        assert_eq!(build_themed_scene(&restored).unwrap(), themed.scene);
    }
}
#[test]
fn fresh_rows_and_baselines_are_checked_before_replay_or_atomic_refresh() {
    for kind in ["scatter", "line", "area", "bar"] {
        let mut mir = compile(&source(kind, true)).unwrap().mir;
        set_row(&mut mir, "y", json!(7.0));
        assert!(build_scene(&mir).is_err());
        let refreshed = rematerialize_mir(&mir).unwrap();
        assert_eq!(chart(&mir).scales, chart(&refreshed).scales);
        assert!(build_scene(&refreshed).is_ok());
        for bad in [f64::from_bits(10.0f64.to_bits() + 1), -2.0] {
            let mut changed = mir.clone();
            set_row(&mut changed, "y", json!(bad));
            let before = serde_json::to_vec(&changed).unwrap();
            assert!(
                build_scene(&changed)
                    .unwrap_err()
                    .to_string()
                    .contains("VIZ-DOMAIN-0002")
            );
            assert!(
                rematerialize_mir(&changed)
                    .unwrap_err()
                    .to_string()
                    .contains("VIZ-DOMAIN-0002")
            );
            assert_eq!(serde_json::to_vec(&changed).unwrap(), before);
        }
        if kind != "bar" {
            set_row(&mut mir, "x", json!(-f64::from_bits(1)));
            assert!(rematerialize_mir(&mir).is_err());
        }
    }
    let mut v = serde_json::to_value(compile(&source("area", true)).unwrap().mir).unwrap();
    v["views"][0]["mark"]["baseline"] = json!(11);
    let mir: VizMir = serde_json::from_value(v).unwrap();
    assert!(build_scene(&mir).is_err());
    assert!(rematerialize_mir(&mir).is_err());
}
#[test]
fn policy_is_versioned_strict_and_for_numeric_position_scales_only() {
    let v = serde_json::to_value(compile(&source("line", true)).unwrap().mir).unwrap();
    for ver in ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6"] {
        let mut bad = v.clone();
        bad["version"] = ver.into();
        bad["source_hir_version"] = ver.into();
        assert!(serde_json::from_value::<VizMir>(bad).is_err());
    }
    for policy in [
        Value::Null,
        json!("clip"),
        json!("extrapolate"),
        json!(true),
        json!({}),
    ] {
        let mut bad = v.clone();
        bad["views"][0]["scales"][0]["out_of_domain"] = policy;
        assert!(serde_json::from_value::<VizMir>(bad).is_err());
    }
    for domain in [json!([1, 1]), json!([2, 1]), json!([-1e308, 1e308])] {
        let mut bad = v.clone();
        bad["views"][0]["scales"][0]["domain"] = domain;
        assert!(serde_json::from_value::<VizMir>(bad).is_err());
    }
    let mut bad = v.clone();
    bad["source_hir_version"] = "0.6".into();
    assert!(serde_json::from_value::<VizMir>(bad).is_err());
    let mut bad = v;
    bad["views"][0]["scales"][0]["id"] = "unused".into();
    assert!(serde_json::from_value::<VizMir>(bad).is_err());
}
#[test]
fn omitted_domains_keep_legacy_mir_scene_and_extrapolation_semantics() {
    for kind in ["scatter", "line", "area", "bar"] {
        let d = source(kind, false);
        let current = compile(&d).unwrap();
        assert!(
            !serde_json::to_string(&current.mir)
                .unwrap()
                .contains("out_of_domain")
        );
        for version in ["0.3", "0.4", "0.5", "0.6"] {
            let mut old = d.clone();
            old.version = version.into();
            let legacy = compile(&old).unwrap();
            assert_eq!(legacy.scene, current.scene);
            let mut expected = current.mir.clone();
            expected.version = version.into();
            expected.source_hir_version = version.into();
            assert_eq!(
                serde_json::to_vec(&legacy.mir).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
        }
        let mut changed = current.mir;
        set_row(&mut changed, "y", json!(100.0));
        assert!(rematerialize_mir(&changed).is_ok());
    }
}
#[test]
fn shared_two_panel_domains_align_same_beta_values_and_keep_stable_colors() {
    let c: Composition = vizir_core::parse_versioned_composition(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/composition/shared-numeric-domains.compose.yaml"),
    )
    .unwrap();
    let d = compose_versioned(&c).unwrap();
    let c = compile(&d).unwrap();
    for id in ["b0", "b1"] {
        let SceneNode::Circle { center: a, .. } =
            find_scene_node(&c.scene.nodes, &format!("all-series/point/{id}")).unwrap()
        else {
            panic!()
        };
        let SceneNode::Circle { center: b, .. } =
            find_scene_node(&c.scene.nodes, &format!("beta-only/point/{id}")).unwrap()
        else {
            panic!()
        };
        assert_eq!(a.y, b.y);
        assert!(b.x > a.x);
    }
    for view in &c.mir.views {
        let MirView::Chart(c) = view else { panic!() };
        let MirScale::Linear { domain, .. } = &c.scales[1] else {
            panic!()
        };
        assert_eq!(*domain, [0.0, 10.0]);
    }
}
#[test]
fn tiny_and_extreme_valid_bounds_are_finite_and_area_projection_collapse_rejects() {
    for domain in [
        [0.0, f64::from_bits(1)],
        [-f64::from_bits(1), f64::from_bits(1)],
        [-1e308, 0.0],
    ] {
        let mut v = serde_json::to_value(source("scatter", true)).unwrap();
        v["views"][0]["y"]["domain"] = json!(domain);
        for (i, row) in v["datasets"]["d"]["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            row["y"] = json!(domain[i]);
        }
        let d = serde_json::from_value(v).unwrap();
        let c = compile(&d).unwrap();
        let s = serde_json::to_string(&c.scene).unwrap();
        assert!(!s.contains("NaN") && !s.contains("Infinity"));
    }
    let mut v = serde_json::to_value(source("area", true)).unwrap();
    v["views"][0]["x"]["domain"] = json!([0.0, 1e308]);
    assert!(lower_to_mir(&serde_json::from_value(v).unwrap()).is_err());
}
#[test]
fn authored_tick_endpoints_survive_large_span_cancellation() {
    let (context, resources) = fixtures::profile();
    for measured in [false, true] {
        let mut v = serde_json::to_value(source("scatter", true)).unwrap();
        v["views"][0]["x"]["domain"] = json!([-1e16, 1.0]);
        v["views"][0]["y"]["domain"] = json!([1.0, 1e16]);
        let d = serde_json::from_value(v).unwrap();
        let scene = if measured {
            compile_with_context(&d, &context, &resources)
                .unwrap()
                .scene
        } else {
            compile(&d).unwrap().scene
        };
        for id in ["chart/axis/x/label/5", "chart/axis/y/label/5"] {
            match find_scene_node(&scene.nodes, id).unwrap() {
                SceneNode::Text { text, .. } => assert_eq!(text, "1"),
                SceneNode::Path { origin, .. } => {
                    assert!(origin.explanation.ends_with("original text: 1"))
                }
                _ => panic!("numeric tick text or measured outline"),
            }
        }
    }
}
#[test]
fn ordinary_authored_ticks_are_concise_without_interpolation_residue() {
    let mut v = serde_json::to_value(source("scatter", true)).unwrap();
    v["views"][0]["x"]["domain"] = json!([-2.0, 10.0]);
    v["views"][0]["y"]["domain"] = json!([0.0, 100.0]);
    v["views"][0]["frame"]["width"] = json!(720);
    let c = compile(&serde_json::from_value(v).unwrap()).unwrap();
    for (index, expected) in ["-2", "0.4", "2.8", "5.2", "7.6", "10"].iter().enumerate() {
        let SceneNode::Text { text, .. } =
            find_scene_node(&c.scene.nodes, &format!("chart/axis/x/label/{index}")).unwrap()
        else {
            panic!()
        };
        assert_eq!(text, expected);
    }
}
#[test]
fn either_numeric_axis_can_opt_in_independently() {
    let original = compile(&source("scatter", false)).unwrap();
    for authored in ["x", "y"] {
        let mut v = serde_json::to_value(source("scatter", false)).unwrap();
        v["views"][0][authored]["domain"] = json!([-1.0, 10.0]);
        let compiled = compile(&serde_json::from_value(v).unwrap()).unwrap();
        for (before, after) in chart(&original.mir)
            .scales
            .iter()
            .zip(&chart(&compiled.mir).scales)
        {
            if let (
                MirScale::Linear { domain: old, .. },
                MirScale::Linear {
                    id,
                    domain,
                    out_of_domain,
                    ..
                },
            ) = (before, after)
            {
                if id.ends_with(&format!("/{authored}")) {
                    assert_eq!(*domain, [-1.0, 10.0]);
                    assert_eq!(*out_of_domain, Some(NumericOutOfDomain::Reject));
                } else {
                    assert_eq!(domain, old);
                    assert!(out_of_domain.is_none());
                }
            }
        }
    }
}
