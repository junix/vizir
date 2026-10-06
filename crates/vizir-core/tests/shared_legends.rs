use serde_json::{Value, json};
use vizir_core::{
    Composition, CompositionVersion, Document, Frame, MirView, View, VizMir, compose_versioned,
    composition_schema, mir_schema, validate_document, validate_mir,
};

fn composition() -> Value {
    json!({"schema":"vizir-composition/0.7","id":"shared","width":1000,"height":800,
    "datasets":{"d":{"key":"id","rows":[{"id":"a","x":1,"y":2,"group":" Beta "}]}},
    "layout":{"kind":"grid","columns":2,"gap":20,"padding":30},
    "shared_legend":{"id":"categories","members":["line","scatter"],"placement":"bottom","height":60,"gap":20,"title":"Category"},
    "panels":[
        {"kind":"chart.line","id":"line","dataset":"d","x":{"field":"x","domain":[0,2]},"y":{"field":"y"},"series":{"field":"group","domain":[" Beta ","α","absent"]}},
        {"kind":"chart.scatter","id":"scatter","dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"group","domain":[" Beta ","α","absent"]}},
        {"kind":"geometry.scene","id":"nonmember","children":[]}
    ]})
}

fn hir() -> Document {
    compose_versioned(&serde_json::from_value(composition()).unwrap()).unwrap()
}

fn mir() -> Value {
    let chart = json!({"dialect":"chart","id":"area","title":null,"frame":{"x":0,"y":0,"width":300,"height":300},"space":"plot","source":"data","row_variable":"row","key_expression":"key",
        "scales":[
            {"type":"linear","id":"x","domain":[0,2],"range":[0,300],"range_space":"plot","zero":false,"out_of_domain":"reject"},
            {"type":"linear","id":"y","domain":[0,3],"range":[300,0],"range_space":"plot","zero":true},
            {"type":"ordinal-color","id":"color","domain":[" Beta ","α","absent"],"range":["#112233","#334455","#556677"]}
        ],
        "guides":[
            {"id":"x-axis","kind":"axis","scale":"x","label":"X","orient":"bottom"},
            {"id":"y-axis","kind":"axis","scale":"y","label":"Y","orient":"left"}
        ],
        "mark":{"type":"area","id":"area-mark","x":{"scale":"x","expression":"x"},"y":{"scale":"y","expression":"y"},"color":{"scale":"color","expression":"group"},"group_expression":"group","order_expression":"x","baseline":0,"series":[]},"provenance":[]});
    let mut second = chart.clone();
    second["id"] = "second".into();
    second["frame"]["x"] = 320.into();
    second["mark"]["id"] = "second-mark".into();
    json!({"version":"0.8","source_hir_version":"0.8","document_id":"shared","width":640,"height":400,"background":"transparent",
        "spaces":{"plot":{"id":"plot","kind":"plot","unit":"scene-unit","transform_to_parent":{}}},
        "data":{"data":{"id":"data","schema":{"key":"id","fields":{"id":{"type":"string"},"x":{"type":"float64"},"y":{"type":"float64"},"group":{"type":"string"}}},"operator":{"kind":"inline","rows":[]},"update_mode":"replace","deterministic":true}},
        "expressions":{
            "key":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"id"}},
            "x":{"result_type":{"type":"float64"},"expression":{"op":"field","row":"row","field":"x"}},
            "y":{"result_type":{"type":"float64"},"expression":{"op":"field","row":"row","field":"y"}},
            "group":{"result_type":{"type":"string"},"expression":{"op":"field","row":"row","field":"group"}}
        },"views":[chart,second],"losses":[],
        "shared_legend":{"id":"categories","placement":"bottom","frame":{"x":20,"y":330,"width":600,"height":50},"title":"Category","members":[{"view":"area","scale":"color"},{"view":"second","scale":"color"}]}
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
fn local_legend() -> Value {
    json!({"id":"legend","kind":"legend","scale":"color","label":"Category","orient":"right"})
}

#[test]
fn composition_reserves_fixed_strip_before_partial_grid_and_preserves_order() {
    let document = hir();
    assert_eq!(document.version, "0.8");
    let owner = document.shared_legend.as_ref().unwrap();
    assert_eq!(
        owner.frame,
        Frame {
            x: 30.,
            y: 710.,
            width: 940.,
            height: 60.
        }
    );
    assert_eq!(owner.members, ["line", "scatter"]);
    assert_eq!(
        *document.views[0].frame(),
        Frame {
            x: 30.,
            y: 30.,
            width: 460.,
            height: 320.
        }
    );
    assert_eq!(
        *document.views[2].frame(),
        Frame {
            x: 30.,
            y: 370.,
            width: 460.,
            height: 320.
        }
    );
    let View::Line(line) = &document.views[0] else {
        panic!()
    };
    assert_eq!(
        line.series.as_ref().unwrap().domain.as_ref().unwrap(),
        &[" Beta ", "α", "absent"]
    );
    let yaml = serde_yaml::to_string(&composition()).unwrap();
    assert_eq!(
        compose_versioned(&serde_yaml::from_str(&yaml).unwrap()).unwrap(),
        document
    );
    assert_eq!(
        serde_json::from_value::<Document>(serde_json::to_value(&document).unwrap()).unwrap(),
        document
    );
}

#[test]
fn root_ownership_is_versioned_closed_nonnull_and_absence_is_omitted() {
    for version in ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "0.7", "future"] {
        let mut value = serde_json::to_value(hir()).unwrap();
        value["version"] = version.into();
        assert!(serde_json::from_value::<Document>(value.clone()).is_err());
        value["shared_legend"] = Value::Null;
        assert!(serde_json::from_value::<Document>(value.clone()).is_err());
        assert!(serde_yaml::from_str::<Document>(&serde_yaml::to_string(&value).unwrap()).is_err());
        let mut value = mir();
        value["version"] = version.into();
        value["source_hir_version"] = version.into();
        assert!(serde_json::from_value::<VizMir>(value.clone()).is_err());
        value["shared_legend"] = Value::Null;
        assert!(serde_json::from_value::<VizMir>(value).is_err());
    }
    for version in ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6"] {
        let mut value = composition();
        value["schema"] = format!("vizir-composition/{version}").into();
        assert!(serde_json::from_value::<Composition>(value.clone()).is_err());
        value["shared_legend"] = Value::Null;
        assert!(serde_json::from_value::<Composition>(value).is_err());
    }
    for value in [Value::Null, json!({}), json!([])] {
        let mut source = composition();
        source["shared_legend"] = value;
        assert!(serde_json::from_value::<Composition>(source).is_err());
    }
    for key in [
        "domain",
        "range",
        "palette",
        "font_size",
        "orient",
        "unknown",
    ] {
        let mut source = composition();
        source["shared_legend"][key] = json!([]);
        assert!(serde_json::from_value::<Composition>(source).is_err());
        let mut source = mir();
        source["shared_legend"][key] = json!([]);
        assert!(serde_json::from_value::<VizMir>(source).is_err());
    }
    let mut source: Composition = serde_json::from_value(composition()).unwrap();
    source.shared_legend = None;
    assert!(
        serde_json::to_value(&source)
            .unwrap()
            .get("shared_legend")
            .is_none()
    );
    let document = compose_versioned(&source).unwrap();
    assert!(
        serde_json::to_value(document)
            .unwrap()
            .get("shared_legend")
            .is_none()
    );
    let mut source: VizMir = serde_json::from_value(mir()).unwrap();
    source.shared_legend = None;
    assert!(
        serde_json::to_value(source)
            .unwrap()
            .get("shared_legend")
            .is_none()
    );
}

#[test]
fn members_require_supported_colored_views_explicit_equal_ordered_domains() {
    for members in [
        json!([]),
        json!(["line"]),
        json!(["line", "line"]),
        json!(["line", "unknown"]),
        json!(["line", "nonmember"]),
        json!(["line", 1]),
        json!(["line", true]),
        json!(["line", ""]),
        json!(["line", "bad: id"]),
        json!(["line", "x".repeat(16385)]),
        json!((0..65).map(|i| format!("v{i}")).collect::<Vec<_>>()),
    ] {
        let mut value = composition();
        value["shared_legend"]["members"] = members;
        assert!(!valid_composition(value.clone()), "{value}");
        if let Ok(parsed) =
            serde_yaml::from_str::<Composition>(&serde_yaml::to_string(&value).unwrap())
        {
            assert!(compose_versioned(&parsed).is_err());
        }
    }
    for domain in [
        Value::Null,
        json!(["α", " Beta ", "absent"]),
        json!(["Beta", "α", "absent"]),
        json!([" Beta ", "α"]),
        json!([" Beta ", "α", "absent", "extra"]),
    ] {
        let mut value = composition();
        value["panels"][1]["color"]["domain"] = domain;
        assert!(!valid_composition(value));
    }
    for key in ["domain", "field"] {
        let mut value = composition();
        value["panels"][1]["color"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(!valid_composition(value));
    }
    let mut value = composition();
    value["panels"][1].as_object_mut().unwrap().remove("color");
    assert!(!valid_composition(value));
    for id in ["", "line", "bad: id", &"x".repeat(16385)] {
        let mut value = composition();
        value["shared_legend"]["id"] = id.into();
        assert!(!valid_composition(value));
    }
}

#[test]
fn all_supported_members_and_prior_numeric_capabilities_remain_available() {
    let mut value = composition();
    value["panels"][0]["kind"] = "chart.area".into();
    value["panels"][0]["baseline"] = 0.into();
    value["panels"][0]["order"] = "x-ascending".into();
    let mut bar = value["panels"][1].clone();
    bar["kind"] = "chart.bar".into();
    bar["id"] = "bar".into();
    bar.as_object_mut().unwrap().remove("x");
    bar.as_object_mut().unwrap().remove("y");
    bar["category"] = json!({"field":"id"});
    bar["value"] = json!({"field":"y","domain":[0,5]});
    value["panels"][2] = bar;
    value["shared_legend"]["members"] = json!(["line", "scatter", "bar"]);
    assert!(valid_composition(value));
    let mut typed: Composition = serde_json::from_value(composition()).unwrap();
    typed.schema = CompositionVersion::V6;
    assert!(compose_versioned(&typed).is_err());
    let mut typed = hir();
    typed.version = "0.7".into();
    assert!(validate_document(&typed).is_err());
}

#[test]
fn layout_rejects_bad_sizes_unrepresentable_gaps_and_overlap_with_nonmembers() {
    for (field, value) in [
        ("height", json!(0)),
        ("height", json!(-1)),
        ("height", json!(800)),
        ("height", json!(1e308)),
        ("gap", json!(-1)),
        ("gap", json!(800)),
        ("gap", json!(1e-300)),
    ] {
        let mut source = composition();
        source["shared_legend"][field] = value;
        assert!(!valid_composition(source), "{field}");
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut source: Composition = serde_json::from_value(composition()).unwrap();
        source.shared_legend.as_mut().unwrap().height = bad;
        assert!(compose_versioned(&source).is_err());
    }
    for (field, value) in [
        ("x", json!(-1)),
        ("y", json!(800)),
        ("width", json!(1001)),
        ("height", json!(0)),
        ("height", json!(100)),
    ] {
        let mut source = serde_json::to_value(hir()).unwrap();
        source["shared_legend"]["frame"][field] = value;
        assert!(!valid_hir(source));
    }
    let mut source = serde_json::to_value(hir()).unwrap();
    source["views"][2]["frame"]["y"] = 700.into();
    source["views"][2]["frame"]["x"] = 2000.into();
    assert!(!valid_hir(source));
    let mut source = hir();
    source.shared_legend.as_mut().unwrap().frame.x = f64::NAN;
    assert!(validate_document(&source).is_err());
    let mut source = composition();
    source["shared_legend"]["gap"] = 0.into();
    assert!(valid_composition(source));
}

#[test]
fn title_and_placement_are_strict_and_single_line() {
    for title in [
        Value::Null,
        json!(1),
        json!(""),
        json!("a\nb"),
        json!("a\tb"),
        json!("a\u{2028}b"),
        json!("a\u{2029}b"),
        json!("x".repeat(16385)),
        json!("界".repeat(5462)),
    ] {
        let mut value = composition();
        value["shared_legend"]["title"] = title;
        assert!(!valid_composition(value));
    }
    for placement in [
        Value::Null,
        json!("right"),
        json!("top"),
        json!("left"),
        json!("Bottom"),
    ] {
        let mut value = composition();
        value["shared_legend"]["placement"] = placement;
        assert!(!valid_composition(value));
    }
    let mut value = composition();
    value["shared_legend"]
        .as_object_mut()
        .unwrap()
        .remove("title");
    assert!(valid_composition(value));
    let mut value = composition();
    value["shared_legend"]["title"] = ("界".repeat(5461) + "a").into();
    assert!(valid_composition(value));
}

#[test]
fn mir_owner_has_only_member_scale_references_and_roundtrips() {
    let value = mir();
    assert!(valid_mir(value.clone()));
    let typed: VizMir = serde_json::from_value(value).unwrap();
    let canonical = serde_json::to_value(&typed).unwrap();
    assert!(canonical["shared_legend"].get("domain").is_none());
    assert!(canonical["shared_legend"].get("range").is_none());
    assert_eq!(typed, serde_json::from_value(canonical).unwrap());
    assert_eq!(
        typed,
        serde_yaml::from_str(&serde_yaml::to_string(&typed).unwrap()).unwrap()
    );
    for version in ["0.7", "future"] {
        let mut typed = typed.clone();
        typed.version = version.into();
        assert!(validate_mir(&typed).is_err());
    }
}

#[test]
fn mir_rejects_local_ownership_wrong_scale_dangling_and_mapping_mutations() {
    for (path, value) in [
        ("/shared_legend/members/1/view", json!("unknown")),
        ("/shared_legend/members/1/view", json!("area")),
        ("/shared_legend/members/0/scale", json!("unknown")),
        ("/shared_legend/members/0/scale", json!("x")),
        ("/shared_legend/members/0/scale", json!("bad:id")),
        ("/shared_legend/id", json!("area")),
        ("/views/1/scales/2/domain", json!(["α", " Beta ", "absent"])),
        (
            "/views/1/scales/2/range",
            json!(["#112233", "#334455", "#556678"]),
        ),
        ("/views/1/scales/2/range", json!(["#112233"])),
        (
            "/views/1/scales/2/range",
            json!(["#112233", "#334455", "bad"]),
        ),
        ("/views/1/mark/color", Value::Null),
    ] {
        let mut source = mir();
        *source.pointer_mut(path).unwrap() = value;
        assert!(!valid_mir(source), "{path}");
    }
    let mut source = mir();
    source["views"][0]["guides"]
        .as_array_mut()
        .unwrap()
        .push(local_legend());
    assert!(!valid_mir(source));
    let mut source = mir();
    source["views"][0]["scales"].as_array_mut().unwrap().push(json!({"type":"ordinal-color","id":"other","domain":[" Beta ","α","absent"],"range":["#112233","#334455","#556677"]}));
    source["shared_legend"]["members"][0]["scale"] = "other".into();
    assert!(!valid_mir(source));
    let mut source = mir();
    source["shared_legend"]["members"] = json!([]);
    assert!(!valid_mir(source));
    let mut source = mir();
    source["shared_legend"]["members"][0]["extra"] = true.into();
    assert!(!valid_mir(source));
}

#[test]
fn mir_area_exception_requires_membership_and_nonmember_strip_is_reserved() {
    let mut source = mir();
    let mut nonmember = source["views"][0].clone();
    nonmember["id"] = "nonmember".into();
    nonmember["guides"]
        .as_array_mut()
        .unwrap()
        .push(local_legend());
    source["views"].as_array_mut().unwrap().push(nonmember);
    assert!(valid_mir(source.clone()));
    source["views"][2]["guides"].as_array_mut().unwrap().pop();
    assert!(!valid_mir(source.clone()));
    source["views"][2]["guides"]
        .as_array_mut()
        .unwrap()
        .push(local_legend());
    source["views"][2]["frame"]["y"] = 300.into();
    source["views"][2]["frame"]["x"] = 1000.into();
    assert!(!valid_mir(source));
    let mut source = mir();
    source.as_object_mut().unwrap().remove("shared_legend");
    assert!(!valid_mir(source.clone()));
    for view in source["views"].as_array_mut().unwrap() {
        view["guides"].as_array_mut().unwrap().push(local_legend());
    }
    assert!(valid_mir(source));
}

#[test]
fn mir_domain_bounds_and_nonfinite_frames_are_checked_in_typed_api() {
    for domain in [
        json!([]),
        json!(["a", "a"]),
        json!(["a\nb"]),
        json!(["a\u{2028}b"]),
        json!(["x".repeat(16385)]),
        json!((0..257).map(|i| i.to_string()).collect::<Vec<_>>()),
    ] {
        let mut source = mir();
        source["views"][0]["scales"][2]["domain"] = domain;
        assert!(!valid_mir(source));
    }
    let mut source: VizMir = serde_json::from_value(mir()).unwrap();
    source.shared_legend.as_mut().unwrap().frame.y = f64::NAN;
    assert!(validate_mir(&source).is_err());
    let mut source: VizMir = serde_json::from_value(mir()).unwrap();
    let MirView::Chart(chart) = &mut source.views[0] else {
        panic!()
    };
    chart.frame.height = f64::INFINITY;
    assert!(validate_mir(&source).is_err());
}

#[test]
fn all_pre08_schema_closures_are_frozen_and_new_root_contract_is_closed() {
    let frozen: Value = serde_json::from_str(include_str!(
        "fixtures/shared-legend-frozen-definitions.json"
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
        c["$defs"]["CompositionV7"]["properties"]["schema"]["const"],
        "vizir-composition/0.7"
    );
    assert_eq!(
        m["$defs"]["VizMirV08"]["properties"]["version"]["const"],
        "0.8"
    );
    assert_eq!(
        m["$defs"]["VizMirV08"]["properties"]["source_hir_version"]["const"],
        "0.8"
    );
    for (schema, name) in [
        (&c, "CompositionSharedLegend"),
        (&m, "MirSharedLegend"),
        (&m, "MirSharedLegendMember"),
    ] {
        assert_eq!(schema["$defs"][name]["additionalProperties"], false);
    }
    assert_eq!(
        c["$defs"]["CompositionSharedLegend"]["properties"]["members"]["maxItems"],
        64
    );
    assert_eq!(
        c["$defs"]["SharedLegendPlacement"]["enum"],
        json!(["bottom"])
    );
    assert_eq!(
        m["$defs"]["GuideOrient"]["enum"],
        json!(["bottom", "left", "right"])
    );
    assert!(
        c["$defs"]["CompositionV6"]["properties"]
            .get("shared_legend")
            .is_none()
    );
    assert!(
        m["$defs"]["VizMirV07"]["properties"]
            .get("shared_legend")
            .is_none()
    );
}

#[test]
fn yaml_owner_ids_and_scale_references_never_coerce_numbers_or_booleans() {
    for id in [json!(123), json!(true)] {
        let mut source = composition();
        source["shared_legend"]["id"] = id.clone();
        assert!(
            serde_yaml::from_str::<Composition>(&serde_yaml::to_string(&source).unwrap()).is_err()
        );
        let mut source = serde_json::to_value(hir()).unwrap();
        source["shared_legend"]["id"] = id.clone();
        assert!(
            serde_yaml::from_str::<Document>(&serde_yaml::to_string(&source).unwrap()).is_err()
        );
        for pointer in [
            "/shared_legend/id",
            "/shared_legend/members/0/view",
            "/shared_legend/members/0/scale",
        ] {
            let mut source = mir();
            *source.pointer_mut(pointer).unwrap() = id.clone();
            assert!(
                serde_yaml::from_str::<VizMir>(&serde_yaml::to_string(&source).unwrap()).is_err()
            );
        }
    }
}
