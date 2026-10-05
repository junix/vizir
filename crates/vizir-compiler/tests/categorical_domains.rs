#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
use serde_json::json;
use vizir_compiler::{
    THEME_NAMES, build_compiled_scene, build_scene, compile, compile_with_context,
    compile_with_theme, parse_compiled_mir_json, rematerialize_compiled_mir, rematerialize_mir,
};
use vizir_core::{Document, MirDataOperator, MirScale, MirView, VizMir};

fn source(kind: &str, subset: bool, explicit: bool) -> Document {
    let mut view = json!({"kind":format!("chart.{kind}"),"id":"chart","frame":{"x":0,"y":0,"width":900,"height":550},"dataset":"d"});
    if kind == "bar" {
        view["category"] = json!({"field":"id"});
        view["value"] = json!({"field":"y"});
    } else {
        view["x"] = json!({"field":"x"});
        view["y"] = json!({"field":"y"});
    }
    if kind == "area" {
        view["baseline"] = json!(0);
        view["order"] = json!("x-ascending");
    }
    let encoding = if matches!(kind, "line" | "area") {
        "series"
    } else {
        "color"
    };
    view[encoding] = json!({"field":"g"});
    if explicit {
        view[encoding]["domain"] = json!(["Beta", "Alpha", "Reserved"]);
    }
    let mut rows = json!([{"id":"a0","x":0,"y":2,"g":"Alpha"},{"id":"a1","x":1,"y":3,"g":"Alpha"},{"id":"b0","x":0,"y":4,"g":"Beta"},{"id":"b1","x":1,"y":5,"g":"Beta"}]);
    if subset {
        rows.as_array_mut().unwrap().drain(0..2);
    }
    serde_json::from_value(json!({"version":"0.6","id":"colors","width":900,"height":550,"datasets":{"d":{"key":"id","rows":rows}},"views":[view]})).unwrap()
}
fn scale(mir: &VizMir) -> &MirScale {
    let MirView::Chart(chart) = &mir.views[0] else {
        panic!()
    };
    chart
        .scales
        .iter()
        .find(|s| matches!(s, MirScale::OrdinalColor { .. }))
        .unwrap()
}
fn painted_keys(mir: &VizMir) -> Vec<String> {
    let value = serde_json::to_value(mir).unwrap();
    let mark = &value["views"][0]["mark"];
    if let Some(series) = mark["series"].as_array() {
        series
            .iter()
            .map(|s| s["key"].as_str().unwrap().to_owned())
            .collect()
    } else {
        mark["instances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap().to_owned())
            .collect()
    }
}

#[test]
fn all_four_kinds_preserve_order_absent_categories_rows_keys_and_replay() {
    for kind in ["scatter", "line", "area", "bar"] {
        let full = compile(&source(kind, false, true)).unwrap();
        let d = source(kind, true, true);
        let subset = compile(&d).unwrap();
        assert_eq!(scale(&full.mir), scale(&subset.mir), "{kind}");
        let MirScale::OrdinalColor { domain, range, .. } = scale(&subset.mir) else {
            panic!()
        };
        assert_eq!(domain, &["Beta", "Alpha", "Reserved"]);
        assert_eq!(range[0].0, "#3B6EF5");
        assert_eq!(range[1].0, "#EB5E55");
        assert_eq!(subset.mir.version, "0.6");
        assert_eq!(subset.mir.source_hir_version, "0.6");
        let MirDataOperator::Inline { rows } = &subset.mir.data["data/d"].operator;
        assert_eq!(rows, &d.datasets["d"].rows);
        let expected = if matches!(kind, "line" | "area") {
            vec!["Beta"]
        } else {
            vec!["b0", "b1"]
        };
        assert_eq!(painted_keys(&subset.mir), expected);
        assert_eq!(build_scene(&subset.mir).unwrap(), subset.scene);
        assert_eq!(rematerialize_mir(&subset.mir).unwrap(), subset.mir);
        let mut reordered = d.clone();
        reordered.datasets.get_mut("d").unwrap().rows.reverse();
        assert_eq!(scale(&compile(&reordered).unwrap().mir), scale(&subset.mir));
        let mut refresh = full.mir.clone();
        let MirDataOperator::Inline { rows } =
            &mut refresh.data.get_mut("data/d").unwrap().operator;
        rows.retain(|row| row["g"] == "Beta");
        let refreshed = rematerialize_mir(&refresh).unwrap();
        assert_eq!(scale(&refreshed), scale(&full.mir));
        assert_eq!(painted_keys(&refreshed), expected);
        let MirDataOperator::Inline { rows } =
            &mut refresh.data.get_mut("data/d").unwrap().operator;
        rows[0].insert("g".into(), "Other".into());
        assert!(rematerialize_mir(&refresh).is_err());
    }
}

#[test]
fn default_theme_and_short_custom_palettes_cycle_in_declared_order() {
    for kind in ["scatter", "line", "area", "bar"] {
        for name in THEME_NAMES {
            let full = compile_with_theme(&source(kind, false, true), name).unwrap();
            let subset = compile_with_theme(&source(kind, true, true), name).unwrap();
            assert_eq!(scale(&full.mir.mir), scale(&subset.mir.mir));
        }
        let mut authored = serde_json::to_value(source(kind, true, true)).unwrap();
        let encoding = if matches!(kind, "line" | "area") {
            "series"
        } else {
            "color"
        };
        authored["views"][0][encoding]["palette"] = json!(["#112233", "#445566"]);
        let compiled = compile(&serde_json::from_value(authored).unwrap()).unwrap();
        let MirScale::OrdinalColor { range, .. } = scale(&compiled.mir) else {
            panic!()
        };
        assert_eq!(
            range.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["#112233", "#445566", "#112233"]
        );
    }
}

#[test]
fn compiled_measured_context_roundtrips_and_legacy_omissions_are_unchanged() {
    let (context, resources) = fixtures::profile();
    for kind in ["scatter", "line", "area", "bar"] {
        let d = source(kind, true, true);
        let c = compile_with_context(&d, &context, &resources).unwrap();
        let replay = parse_compiled_mir_json(&serde_json::to_vec(&c.mir).unwrap()).unwrap();
        assert_eq!(build_compiled_scene(&replay, &resources).unwrap(), c.scene);
        assert_eq!(
            rematerialize_compiled_mir(&replay, &resources).unwrap(),
            replay
        );
        let new = source(kind, false, false);
        let current = compile(&new).unwrap();
        for version in ["0.3", "0.4", "0.5"] {
            let mut old = new.clone();
            old.version = version.into();
            let legacy = compile(&old).unwrap();
            assert_eq!(legacy.scene, current.scene);
            let mut mir = current.mir.clone();
            mir.version = version.into();
            mir.source_hir_version = version.into();
            assert_eq!(
                serde_json::to_vec(&legacy.mir).unwrap(),
                serde_json::to_vec(&mir).unwrap()
            );
        }
    }
}

#[test]
fn native_version_pair_is_exact_and_scalar_categories_remain_canonical() {
    for kind in ["scatter", "line", "area", "bar"] {
        let mut value =
            serde_json::to_value(compile(&source(kind, true, true)).unwrap().mir).unwrap();
        value["source_hir_version"] = "0.5".into();
        assert!(serde_json::from_value::<VizMir>(value.clone()).is_err());
        for scalar in [
            json!(true),
            json!(9007199254740993_i64),
            json!(1.0),
            json!(-0.0),
        ] {
            let mut authored = serde_json::to_value(source(kind, true, true)).unwrap();
            for row in authored["datasets"]["d"]["rows"].as_array_mut().unwrap() {
                row["g"] = scalar.clone();
            }
            let key = vizir_core::value_as_key(&scalar).unwrap();
            let encoding = if matches!(kind, "line" | "area") {
                "series"
            } else {
                "color"
            };
            authored["views"][0][encoding]["domain"] = json!(["reserved", key]);
            let c = compile(&serde_json::from_value(authored).unwrap()).unwrap();
            let MirScale::OrdinalColor { domain, range, .. } = scale(&c.mir) else {
                panic!()
            };
            assert_eq!(domain[1], key);
            assert_eq!(range[1].0, "#EB5E55");
        }
    }
}
