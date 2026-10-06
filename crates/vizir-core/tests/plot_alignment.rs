use serde_json::{Value, json};
use vizir_core::{
    Composition, CompositionVersion, Document, MirScale, MirView, PlotAlignmentMode, View, VizMir,
    compose_versioned, composition_schema, mir_schema, validate_document, validate_mir,
};

fn composition() -> Value {
    json!({"schema":"vizir-composition/0.8","id":"aligned","width":1001,"height":801,
    "datasets":{"d":{"key":"id","rows":[{"id":"a","x":1,"y":2,"group":"a"}]}},
    "layout":{"kind":"grid","columns":2,"gap":17,"padding":23},
    "plot_alignment":{"id":"plot-grid","mode":"uniform","members":["scatter","line","area"]},
    "panels":[
        {"kind":"chart.line","id":"line","dataset":"d","x":{"field":"x","domain":[0,2],"axis":{"number_format":{"notation":"fixed","precision":1}}},"y":{"field":"y","domain":[0,5]}},
        {"kind":"chart.scatter","id":"scatter","dataset":"d","x":{"field":"x","domain":[-10,20]},"y":{"field":"y","domain":[0,5000]}},
        {"kind":"chart.area","id":"area","dataset":"d","x":{"field":"x"},"y":{"field":"y"},"baseline":0,"order":"x-ascending"},
        {"kind":"geometry.scene","id":"nonmember","children":[]}
    ]})
}

fn hir() -> Document {
    compose_versioned(&serde_json::from_value(composition()).unwrap()).unwrap()
}

fn mir() -> Value {
    let first = json!({"dialect":"chart","id":"first","title":null,
        "frame":{"x":20,"y":20,"width":300,"height":300},"space":"plot","source":"data","row_variable":"row","key_expression":"key",
        "scales":[
            {"type":"linear","id":"x","domain":[0,2],"range":[0,300],"range_space":"plot","zero":false,"out_of_domain":"reject"},
            {"type":"linear","id":"y","domain":[0,3],"range":[300,0],"range_space":"plot","zero":true}
        ],
        "guides":[
            {"id":"x-axis","kind":"axis","scale":"x","label":"X","orient":"bottom"},
            {"id":"y-axis","kind":"axis","scale":"y","label":"Y","orient":"left"}
        ],
        "mark":{"type":"symbol","id":"first-mark","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},"color":null,"size":7,"instances":[]},"provenance":[]});
    let mut second = first.clone();
    second["id"] = "second".into();
    second["frame"]["x"] = 340.into();
    second["mark"]["id"] = "second-mark".into();
    second["scales"][0]["domain"] = json!([-100, 500]);
    second["scales"][1]["domain"] = json!([-0.01, 0.02]);
    json!({"version":"0.9","source_hir_version":"0.9","document_id":"aligned","width":660,"height":400,"background":"transparent",
        "spaces":{"plot":{"id":"plot","kind":"document","unit":"scene-unit","transform_to_parent":{}}},
        "data":{"data":{"id":"data","schema":{"key":"id","fields":{"id":{"type":"string"},"x":{"type":"float64"},"y":{"type":"float64"},"group":{"type":"string"}}},"operator":{"kind":"inline","rows":[]},"update_mode":"replace","deterministic":true}},
        "expressions":{
            "key":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"id"}},
            "x":{"result_type":{"type":"float64"},"expression":{"op":"field","row":"row","field":"x"}},
            "y":{"result_type":{"type":"float64"},"expression":{"op":"field","row":"row","field":"y"}},
            "group":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"group"}}
        },"views":[first,second],"losses":[],
        "plot_alignment":{"id":"plot-grid","mode":"uniform","members":[{"view":"second","x_scale":"x","y_scale":"y"},{"view":"first","x_scale":"x","y_scale":"y"}]}
    })
}

fn valid_composition(value: Value) -> bool {
    serde_json::from_value::<Composition>(value).is_ok_and(|c| compose_versioned(&c).is_ok())
}
fn valid_hir(value: Value) -> bool {
    serde_json::from_value::<Document>(value).is_ok_and(|d| validate_document(&d).is_ok())
}
fn valid_mir(value: Value) -> bool {
    serde_json::from_value::<VizMir>(value).is_ok_and(|m| validate_mir(&m).is_ok())
}

#[test]
fn composition_resolves_member_order_without_changing_frames_or_independent_domains() {
    let source: Composition = serde_json::from_value(composition()).unwrap();
    assert_eq!(source.schema, CompositionVersion::V8);
    let document = compose_versioned(&source).unwrap();
    assert_eq!(document.version, "0.9");
    let group = document.plot_alignment.as_ref().unwrap();
    assert_eq!(group.mode, PlotAlignmentMode::Uniform);
    assert_eq!(group.members, ["scatter", "line", "area"]);
    let mut legacy = source.clone();
    legacy.schema = CompositionVersion::V7;
    legacy.plot_alignment = None;
    let previous = compose_versioned(&legacy).unwrap();
    assert_eq!(document.views, previous.views);
    assert_eq!(document.datasets, previous.datasets);
    let View::Line(line) = &document.views[0] else {
        panic!()
    };
    let View::Scatter(scatter) = &document.views[1] else {
        panic!()
    };
    assert_eq!(line.x.domain, Some([0., 2.]));
    assert_eq!(scatter.x.domain, Some([-10., 20.]));
    assert_eq!(line.y.domain, Some([0., 5.]));
    assert_eq!(scatter.y.domain, Some([0., 5000.]));
    let View::Area(area) = &document.views[2] else {
        panic!()
    };
    assert!(area.x.domain.is_none());
    assert!(area.y.domain.is_none());
    assert_eq!(
        source,
        serde_yaml::from_str(&serde_yaml::to_string(&source).unwrap()).unwrap()
    );
    assert_eq!(
        document,
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap()
    );
}

#[test]
fn root_contract_is_opt_in_versioned_matching_and_nonnull() {
    for version in ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "0.7"] {
        let mut value = composition();
        value["schema"] = format!("vizir-composition/{version}").into();
        assert!(serde_json::from_value::<Composition>(value).is_err());
    }
    for version in [
        "0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "0.7", "0.8", "future",
    ] {
        let mut value = serde_json::to_value(hir()).unwrap();
        value["version"] = version.into();
        assert!(serde_json::from_value::<Document>(value).is_err());
        let mut value = mir();
        value["version"] = version.into();
        value["source_hir_version"] = version.into();
        assert!(serde_json::from_value::<VizMir>(value).is_err());
    }
    for (version, source) in [("0.9", "0.8"), ("0.8", "0.9"), ("0.9", "future")] {
        let mut value = mir();
        value["version"] = version.into();
        value["source_hir_version"] = source.into();
        value.as_object_mut().unwrap().remove("plot_alignment");
        assert!(serde_json::from_value::<VizMir>(value).is_err());
    }
    for group in [Value::Null, json!({}), json!([]), json!(true)] {
        let mut value = composition();
        value["plot_alignment"] = group.clone();
        assert!(serde_json::from_value::<Composition>(value).is_err());
        let mut value = serde_json::to_value(hir()).unwrap();
        value["plot_alignment"] = group.clone();
        assert!(serde_json::from_value::<Document>(value).is_err());
        let mut value = mir();
        value["plot_alignment"] = group;
        assert!(serde_json::from_value::<VizMir>(value).is_err());
    }
    let mut c: Composition = serde_json::from_value(composition()).unwrap();
    c.plot_alignment = None;
    assert!(
        serde_json::to_value(&c)
            .unwrap()
            .get("plot_alignment")
            .is_none()
    );
    let d = compose_versioned(&c).unwrap();
    assert!(
        serde_json::to_value(d)
            .unwrap()
            .get("plot_alignment")
            .is_none()
    );
    let mut m: VizMir = serde_json::from_value(mir()).unwrap();
    m.plot_alignment = None;
    assert!(
        serde_json::to_value(m)
            .unwrap()
            .get("plot_alignment")
            .is_none()
    );
}

#[test]
fn typed_api_cannot_bypass_version_gates() {
    let mut c: Composition = serde_json::from_value(composition()).unwrap();
    c.schema = CompositionVersion::V7;
    assert!(compose_versioned(&c).is_err());
    let mut d = hir();
    d.version = "0.8".into();
    assert!(validate_document(&d).is_err());
    let mut m: VizMir = serde_json::from_value(mir()).unwrap();
    m.source_hir_version = "0.8".into();
    assert!(validate_mir(&m).is_err());
}

#[test]
fn member_ids_and_modes_are_closed_and_never_yaml_coerced() {
    for value in [Value::Null, json!(123), json!(true), json!([]), json!({})] {
        for path in [
            "/plot_alignment/id",
            "/plot_alignment/members/0",
            "/plot_alignment/mode",
        ] {
            let mut source = composition();
            *source.pointer_mut(path).unwrap() = value.clone();
            assert!(serde_json::from_value::<Composition>(source.clone()).is_err());
            assert!(
                serde_yaml::from_str::<Composition>(&serde_yaml::to_string(&source).unwrap())
                    .is_err()
            );
        }
        for path in [
            "/plot_alignment/id",
            "/plot_alignment/members/0/view",
            "/plot_alignment/members/0/x_scale",
            "/plot_alignment/members/0/y_scale",
            "/plot_alignment/mode",
        ] {
            let mut source = mir();
            *source.pointer_mut(path).unwrap() = value.clone();
            assert!(serde_json::from_value::<VizMir>(source.clone()).is_err());
            assert!(
                serde_yaml::from_str::<VizMir>(&serde_yaml::to_string(&source).unwrap()).is_err()
            );
        }
    }
    for mode in ["Uniform", "row", "column", "shared", "none", ""] {
        let mut source = composition();
        source["plot_alignment"]["mode"] = mode.into();
        assert!(serde_json::from_value::<Composition>(source).is_err());
    }
    for key in ["domain", "width", "height", "insets", "groups", "unknown"] {
        let mut source = composition();
        source["plot_alignment"][key] = json!(1);
        assert!(serde_json::from_value::<Composition>(source).is_err());
        let mut source = mir();
        source["plot_alignment"][key] = json!(1);
        assert!(serde_json::from_value::<VizMir>(source).is_err());
        let mut source = mir();
        source["plot_alignment"]["members"][0][key] = json!(1);
        assert!(serde_json::from_value::<VizMir>(source).is_err());
    }
}

#[test]
fn members_are_bounded_unique_resolvable_supported_and_have_valid_ids() {
    for members in [
        json!([]),
        json!(["line"]),
        json!(["line", "line"]),
        json!(["line", "missing"]),
        json!(["line", "nonmember"]),
        json!(["line", ""]),
        json!(["line", "bad:id"]),
        json!(["line", "x".repeat(16385)]),
        json!((0..65).map(|i| format!("v{i}")).collect::<Vec<_>>()),
    ] {
        let mut source = composition();
        source["plot_alignment"]["members"] = members;
        assert!(!valid_composition(source));
    }
    for id in ["", "line", "bad id", "界", &"x".repeat(16385)] {
        let mut source = composition();
        source["plot_alignment"]["id"] = id.into();
        assert!(!valid_composition(source));
    }
    let mut source = composition();
    source["panels"][3] = source["panels"][0].clone();
    assert!(!valid_composition(source));
    let mut source = mir();
    source["plot_alignment"]["members"][0]["view"] = "first".into();
    source["plot_alignment"]["members"][0]["x_scale"] = "different".into();
    assert!(!valid_mir(source));
    let mut source = mir();
    source["views"][1]["id"] = "first".into();
    assert!(!valid_mir(source));
}

#[test]
fn only_numeric_xy_charts_can_be_members() {
    let mut source = composition();
    source["panels"][1] = json!({"kind":"chart.bar","id":"scatter","dataset":"d","category":{"field":"id"},"value":{"field":"y"}});
    assert!(!valid_composition(source));
    let mut source = composition();
    source["panels"][1] = json!({"kind":"chart.heatmap","id":"scatter","dataset":"d","x":{"field":"id"},"y":{"field":"group"},"color":{"field":"y"}});
    assert!(!valid_composition(source));
    let mut source = composition();
    source["panels"][1] = json!({"kind":"diagram.graph","id":"scatter","nodes":[],"edges":[]});
    assert!(!valid_composition(source));
    let mut source = mir();
    source["views"][1] = json!({"dialect":"geometry","id":"second","frame":{"x":340,"y":20,"width":300,"height":300},"space":"plot","children":[],"provenance":[]});
    assert!(!valid_mir(source));
}

#[test]
fn equal_cells_allow_only_operand_rounding_and_frames_stay_in_canvas() {
    let mut source = composition();
    source["width"] = 1000.1.into();
    source["height"] = 801.7.into();
    source["layout"]["columns"] = 3.into();
    source["layout"]["gap"] = 13.1.into();
    source["layout"]["padding"] = 23.3.into();
    assert!(valid_composition(source));
    let mut source = mir();
    source["views"][1]["frame"]["width"] = (300.0 + f64::EPSILON * 300.0).into();
    assert!(valid_mir(source));
    for (field, value) in [
        ("width", json!(300.00001)),
        ("height", json!(299.99)),
        ("x", json!(-1)),
        ("x", json!(500)),
        ("y", json!(200)),
        ("width", json!(0)),
        ("height", json!(-1)),
    ] {
        let mut source = mir();
        source["views"][1]["frame"][field] = value;
        assert!(!valid_mir(source), "{field}");
    }
    for field in ["width", "height", "x", "y"] {
        let mut source = serde_json::to_value(hir()).unwrap();
        source["views"][1]["frame"][field] = json!(-1);
        assert!(!valid_hir(source));
    }
    let mut d = hir();
    let View::Line(chart) = &mut d.views[0] else {
        panic!()
    };
    chart.frame.width = f64::NAN;
    assert!(validate_document(&d).is_err());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut m: VizMir = serde_json::from_value(mir()).unwrap();
        let MirView::Chart(chart) = &mut m.views[0] else {
            panic!()
        };
        chart.frame.height = bad;
        assert!(validate_mir(&m).is_err());
    }
}

#[test]
fn fractional_many_column_grids_allow_symmetric_coordinate_rounding() {
    let mut source = composition();
    source["width"] = 10000.3.into();
    source["height"] = 801.7.into();
    source["layout"]["columns"] = 64.into();
    source["layout"]["gap"] = 1.1.into();
    source["layout"]["padding"] = 2.3.into();
    let panel = source["panels"][1].clone();
    source["panels"] = json!(
        (0..64)
            .map(|i| {
                let mut p = panel.clone();
                p["id"] = format!("p{i}").into();
                p
            })
            .collect::<Vec<_>>()
    );
    source["plot_alignment"]["members"] =
        json!((0..64).map(|i| format!("p{i}")).collect::<Vec<_>>());
    assert!(valid_composition(source.clone()));
    source["plot_alignment"]["members"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert!(valid_composition(source));
}

#[test]
fn member_frames_cannot_overlap_but_nonmember_layout_policy_is_unchanged() {
    let mut source = serde_json::to_value(hir()).unwrap();
    source["views"][1]["frame"] = source["views"][0]["frame"].clone();
    assert!(!valid_hir(source));
    let mut source = mir();
    source["views"][1]["frame"]["x"] = 319.into();
    assert!(!valid_mir(source.clone()));
    source["views"][1]["frame"]["x"] = 320.into();
    assert!(valid_mir(source));
    let mut source = serde_json::to_value(hir()).unwrap();
    source["views"][3]["frame"] = source["views"][0]["frame"].clone();
    assert!(valid_hir(source));
}

#[test]
fn mir_preserves_independent_domains_and_exact_member_reference_order() {
    let source = mir();
    assert!(valid_mir(source.clone()));
    let typed: VizMir = serde_json::from_value(source).unwrap();
    let group = typed.plot_alignment.as_ref().unwrap();
    assert_eq!(group.members[0].view, "second");
    assert_eq!(group.members[1].view, "first");
    let canonical = serde_json::to_value(&typed).unwrap();
    assert!(canonical["plot_alignment"].get("domain").is_none());
    assert!(canonical["plot_alignment"].get("range").is_none());
    assert_eq!(
        canonical["views"][0]["scales"][0]["domain"],
        json!([0.0, 2.0])
    );
    assert_eq!(
        canonical["views"][1]["scales"][0]["domain"],
        json!([-100.0, 500.0])
    );
    assert_eq!(typed, serde_json::from_value(canonical).unwrap());
    assert_eq!(
        typed,
        serde_yaml::from_str(&serde_yaml::to_string(&typed).unwrap()).unwrap()
    );
}

#[test]
fn mir_requires_exact_distinct_linear_xy_bindings_and_bottom_left_axes() {
    for (path, value) in [
        ("/plot_alignment/members/0/view", json!("missing")),
        ("/plot_alignment/members/0/x_scale", json!("missing")),
        ("/plot_alignment/members/0/x_scale", json!("y")),
        ("/plot_alignment/members/0/y_scale", json!("x")),
        ("/plot_alignment/members/0/y_scale", json!("bad:id")),
        ("/plot_alignment/id", json!("first")),
        ("/views/1/guides/0/orient", json!("left")),
        ("/views/1/guides/1/orient", json!("right")),
        ("/views/1/guides/1/scale", json!("x")),
    ] {
        let mut source = mir();
        *source.pointer_mut(path).unwrap() = value;
        assert!(!valid_mir(source), "{path}");
    }
    let mut source = mir();
    source["views"][1]["scales"][0] = json!({"type":"band","id":"x","domain":["a"],"range":[0,300],"range_space":"plot","padding_inner":0.1});
    assert!(!valid_mir(source));
    let mut source = mir();
    let extra = source["views"][1]["guides"][0].clone();
    source["views"][1]["guides"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(!valid_mir(source));
    let mut source = mir();
    source["views"][1]["guides"].as_array_mut().unwrap().pop();
    assert!(!valid_mir(source));
    let mut source = mir();
    let mut extra = source["views"][1]["scales"][0].clone();
    extra["id"] = "other-x".into();
    source["views"][1]["scales"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    source["plot_alignment"]["members"][0]["x_scale"] = "other-x".into();
    assert!(!valid_mir(source));
    let mut m: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(c) = &mut m.views[1] else {
        panic!()
    };
    let duplicate = c.scales[0].clone();
    c.scales.push(duplicate);
    assert!(validate_mir(&m).is_err());
    let mut m: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(c) = &mut m.views[1] else {
        panic!()
    };
    let MirScale::Linear { id, .. } = &mut c.scales[0] else {
        panic!()
    };
    *id = "other".into();
    assert!(validate_mir(&m).is_err());
}

#[test]
fn shared_legend_remains_available_in_matching_09_and_ids_do_not_collide() {
    let mut source = composition();
    source["shared_legend"] = json!({"id":"categories","members":["line","scatter"],"placement":"bottom","height":50,"gap":20});
    source["panels"][0]["series"] = json!({"field":"group","domain":["a","b"]});
    source["panels"][1]["color"] = json!({"field":"group","domain":["a","b"]});
    assert!(valid_composition(source.clone()));
    source["plot_alignment"]["id"] = "categories".into();
    assert!(!valid_composition(source));
    let mut source = mir();
    source["shared_legend"] = json!({"id":"categories","members":[{"view":"first","scale":"color"},{"view":"second","scale":"color"}],"placement":"bottom","frame":{"x":20,"y":330,"width":620,"height":50}});
    for view in source["views"].as_array_mut().unwrap() {
        view["scales"].as_array_mut().unwrap().push(json!({"type":"ordinal-color","id":"color","domain":["a","b"],"range":["#112233","#445566"]}));
        view["mark"]["color"] = json!({"scale":"color","expression":"group"});
    }
    assert!(valid_mir(source.clone()));
    source["plot_alignment"]["id"] = "categories".into();
    assert!(!valid_mir(source));
}

#[test]
fn all_published_schema_branches_and_definitions_are_frozen_before_09() {
    let frozen: Value = serde_json::from_str(include_str!(
        "fixtures/plot-alignment-frozen-definitions.json"
    ))
    .unwrap();
    for (kind, schema) in [("composition", composition_schema()), ("mir", mir_schema())] {
        for (name, definition) in frozen[kind]["$defs"].as_object().unwrap() {
            assert_eq!(&schema["$defs"][name], definition, "{kind}::{name}");
        }
        for (i, branch) in frozen[kind]["oneOf"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&schema["oneOf"][i], branch, "{kind}::{i}");
        }
    }
    let c = composition_schema();
    let m = mir_schema();
    assert_eq!(
        c["$defs"]["CompositionV8"]["properties"]["schema"]["const"],
        "vizir-composition/0.8"
    );
    assert_eq!(
        m["$defs"]["VizMirV09"]["properties"]["version"]["const"],
        "0.9"
    );
    assert_eq!(
        m["$defs"]["VizMirV09"]["properties"]["source_hir_version"]["const"],
        "0.9"
    );
    for (schema, name) in [
        (&c, "PlotAlignment"),
        (&m, "MirPlotAlignment"),
        (&m, "MirPlotAlignmentMember"),
    ] {
        assert_eq!(schema["$defs"][name]["additionalProperties"], false);
    }
    for (schema, name) in [(&c, "PlotAlignment"), (&m, "MirPlotAlignment")] {
        assert_eq!(
            schema["$defs"][name]["properties"]["members"]["minItems"],
            2
        );
        assert_eq!(
            schema["$defs"][name]["properties"]["members"]["maxItems"],
            64
        );
        assert_eq!(
            schema["$defs"][name]["properties"]["members"]["uniqueItems"],
            true
        );
        assert_eq!(
            schema["$defs"]["PlotAlignmentMode"]["enum"],
            json!(["uniform"])
        );
    }
    assert!(
        c["$defs"]["CompositionV7"]["properties"]
            .get("plot_alignment")
            .is_none()
    );
    assert!(
        m["$defs"]["VizMirV08"]["properties"]
            .get("plot_alignment")
            .is_none()
    );
}

#[test]
fn aligned_numeric_domains_are_finite_ordered_and_allow_inferred_constants() {
    for domain in [
        [f64::NAN, 3.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 3.0],
        [-1e308, 1e308],
        [3.0, 0.0],
    ] {
        let mut m: VizMir = serde_json::from_value(mir()).unwrap();
        let MirView::Chart(c) = &mut m.views[0] else {
            panic!()
        };
        let MirScale::Linear {
            domain: actual,
            out_of_domain,
            ..
        } = &mut c.scales[1]
        else {
            panic!()
        };
        *actual = domain;
        *out_of_domain = None;
        assert!(validate_mir(&m).is_err());
    }
    let mut m: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(c) = &mut m.views[0] else {
        panic!()
    };
    let MirScale::Linear {
        domain,
        out_of_domain,
        ..
    } = &mut c.scales[0]
    else {
        panic!()
    };
    *domain = [2.0, 2.0];
    *out_of_domain = None;
    assert!(validate_mir(&m).is_ok());
    let MirView::Chart(c) = &mut m.views[0] else {
        panic!()
    };
    let MirScale::Linear { out_of_domain, .. } = &mut c.scales[0] else {
        panic!()
    };
    *out_of_domain = Some(vizir_core::NumericOutOfDomain::Reject);
    assert!(validate_mir(&m).is_err());
}

#[test]
fn required_fields_reject_missing_null_and_duplicate_properties() {
    for key in ["id", "mode", "members"] {
        let mut source = composition();
        source["plot_alignment"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(serde_json::from_value::<Composition>(source).is_err());
        let mut source = mir();
        source["plot_alignment"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(serde_json::from_value::<VizMir>(source).is_err());
    }
    for key in ["view", "x_scale", "y_scale"] {
        let mut source = mir();
        source["plot_alignment"]["members"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(serde_json::from_value::<VizMir>(source).is_err());
    }
    for members in [
        json!([]),
        json!([{"view":"first","x_scale":"x","y_scale":"y"}]),
        json!(
            (0..65)
                .map(|i| json!({"view":format!("v{i}"),"x_scale":"x","y_scale":"y"}))
                .collect::<Vec<_>>()
        ),
    ] {
        let mut source = mir();
        source["plot_alignment"]["members"] = members;
        assert!(!valid_mir(source));
    }
    let source = serde_json::to_string(&composition()).unwrap();
    assert!(
        serde_json::from_str::<Composition>(
            &source.replace("\"mode\":", "\"mode\":\"uniform\",\"mode\":")
        )
        .is_err()
    );
    let source = serde_json::to_string(&mir()).unwrap();
    assert!(
        serde_json::from_str::<VizMir>(
            &source.replace("\"x_scale\":", "\"x_scale\":\"x\",\"x_scale\":")
        )
        .is_err()
    );
    let c = composition_schema();
    assert_eq!(
        c["$defs"]["PlotAlignment"]["properties"]["members"]["items"],
        json!({"type":"string","minLength":1,"maxLength":16384,"pattern":"^[A-Za-z0-9_/-]+$"})
    );
}

#[test]
fn aligned_mir_uses_identity_document_spaces_with_opaque_ids() {
    let mut source = mir();
    let mut space = source["spaces"]
        .as_object_mut()
        .unwrap()
        .remove("plot")
        .unwrap();
    space["id"] = "opaque/frame-space".into();
    source["spaces"]["opaque/frame-space"] = space;
    for view in source["views"].as_array_mut().unwrap() {
        view["space"] = "opaque/frame-space".into();
        for scale in view["scales"].as_array_mut().unwrap() {
            scale["range_space"] = "opaque/frame-space".into();
        }
    }
    assert!(valid_mir(source));
    for (field, value) in [
        ("kind", json!("plot")),
        ("kind", json!("view-local")),
        ("unit", json!("data-unit")),
        ("parent", json!("plot")),
        ("transform_to_parent", json!({"translate":{"x":1,"y":0}})),
        ("transform_to_parent", json!({"scale":{"x":2,"y":1}})),
        ("transform_to_parent", json!({"rotate_degrees":30})),
    ] {
        let mut source = mir();
        source["spaces"]["plot"][field] = value;
        assert!(!valid_mir(source), "{field}");
    }
    let mut source = mir();
    source["spaces"]["other"] =
        json!({"id":"other","kind":"document","unit":"scene-unit","transform_to_parent":{}});
    for scale in [0, 1] {
        let mut bad = source.clone();
        bad["views"][0]["scales"][scale]["range_space"] = "other".into();
        assert!(!valid_mir(bad));
    }
    // Unaligned views retain the existing coordinate-space contract.
    let mut nonmember = source["views"][0].clone();
    nonmember["id"] = "nonmember".into();
    nonmember["space"] = "other".into();
    source["spaces"]["other"]["kind"] = "view-local".into();
    source["spaces"]["other"]["parent"] = "plot".into();
    source["spaces"]["other"]["transform_to_parent"] = json!({"translate":{"x":1,"y":0}});
    source["views"].as_array_mut().unwrap().push(nonmember);
    assert!(valid_mir(source));
}
