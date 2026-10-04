use serde_json::{Value, json};
use vizir_core::{Color, Origin, Rect, Scene2D, SceneNode, Transform2D};
use vizir_web::{
    ExplorerOptions, InteractionContext, LinkedInteractionContext, SelectionGroup, SelectionLinks,
    render_html, render_linked_html,
};
fn origin(id: &str) -> Origin {
    Origin {
        hir_node: id.into(),
        mir_node: id.into(),
        data_key: Some("same-key".into()),
        data_lineage: vec!["same-source".into()],
        generated_by: "test".into(),
        explanation: String::new(),
    }
}
fn scene() -> Scene2D {
    let leaf = |id: &str| SceneNode::Group {
        id: id.into(),
        bounds: Rect::default(),
        origin: origin(id),
        transform: Transform2D::default(),
        opacity: 1.0,
        children: vec![],
    };
    Scene2D {
        document_id: "links".into(),
        width: 600.0,
        height: 400.0,
        background: Color::transparent(),
        losses: vec![],
        nodes: vec![
            SceneNode::Group {
                id: "panel-a".into(),
                bounds: Rect::default(),
                origin: origin("panel-a"),
                transform: Transform2D::default(),
                opacity: 1.0,
                children: vec![leaf("a"), leaf("b")],
            },
            leaf("panel-b"),
            leaf("c"),
            leaf("d"),
        ],
    }
}
fn metadata(html: &str) -> &str {
    html.split("data-vizir-metadata=\"\">\n")
        .nth(1)
        .unwrap()
        .split("\n</script>")
        .next()
        .unwrap()
}
fn spec(source: &Scene2D) -> SelectionLinks {
    let first = render_html(source, &ExplorerOptions::default()).unwrap();
    SelectionLinks {
        format: vizir_web::LINKS_FORMAT.into(),
        document_id: source.document_id.clone(),
        scene_sha256: first.manifest.scene_sha256,
        instance_key: "main".into(),
        groups: vec![
            SelectionGroup {
                id: "z-last".into(),
                members: vec!["d".into(), "c".into()],
            },
            SelectionGroup {
                id: "a-first".into(),
                members: vec!["a".into(), "panel-a".into(), "panel-b".into()],
            },
        ],
    }
}
#[test]
fn exact_snapshot_links_normalize_only_spec_order_and_preserve_ancestry_membership() {
    let source = scene();
    let spec = spec(&source);
    let original = spec.clone();
    let export = render_linked_html(&source, &ExplorerOptions::default(), &spec).unwrap();
    let context = LinkedInteractionContext::parse(metadata(&export.html).as_bytes()).unwrap();
    assert_eq!(spec, original);
    assert_eq!(source, scene());
    assert_eq!(context.link_groups[0].id, "a-first");
    assert_eq!(context.link_groups[0].members, ["panel-a", "a", "panel-b"]);
    assert!(!context.link_groups[0].members.iter().any(|id| id == "b"));
    assert_eq!(context.link_groups[1].members, ["c", "d"]);
    assert_eq!(export.manifest.profile, vizir_web::LINKED_PROFILE);
    assert_eq!(
        metadata(
            &render_linked_html(&source, &ExplorerOptions::default(), &spec)
                .unwrap()
                .html
        ),
        metadata(&export.html)
    );
    // V1 rejects a v2 envelope and even a v2-only field added to valid v1 metadata.
    assert!(InteractionContext::parse(metadata(&export.html).as_bytes()).is_err());
    let v1 = render_html(&source, &ExplorerOptions::default()).unwrap();
    let mut value: Value = serde_json::from_str(metadata(&v1.html)).unwrap();
    value["link_groups"] = json!([]);
    assert!(InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn linked_metadata_readers_require_canonical_order_and_nonnull_arrays() {
    let source = scene();
    let export = render_linked_html(&source, &ExplorerOptions::default(), &spec(&source)).unwrap();
    let valid: Value = serde_json::from_str(metadata(&export.html)).unwrap();
    for mutation in [
        "groups-order",
        "member-order",
        "missing",
        "null",
        "unknown",
        "overlap",
        "missing-member",
    ] {
        let mut value = valid.clone();
        match mutation {
            "groups-order" => value["link_groups"].as_array_mut().unwrap().reverse(),
            "member-order" => value["link_groups"][0]["members"]
                .as_array_mut()
                .unwrap()
                .reverse(),
            "missing" => {
                value.as_object_mut().unwrap().remove("link_groups");
            }
            "null" => value["link_groups"] = Value::Null,
            "unknown" => value["link_groups"][0]["script"] = "no".into(),
            "overlap" => value["link_groups"][1]["members"][0] = "a".into(),
            "missing-member" => value["link_groups"][0]["members"][0] = "not-in-scene".into(),
            _ => unreachable!(),
        }
        assert!(
            LinkedInteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{mutation}"
        );
    }
}
#[test]
fn stale_scene_document_instance_and_absent_members_reject_without_reconciliation() {
    let source = scene();
    let original = spec(&source);
    for mutation in [
        "digest",
        "document",
        "instance",
        "member",
        "overlap",
        "duplicate-group",
        "duplicate-member",
    ] {
        let mut s = original.clone();
        match mutation {
            "digest" => s.scene_sha256 = "0".repeat(64),
            "document" => s.document_id = "another".into(),
            "instance" => s.instance_key = "another".into(),
            "member" => s.groups[0].members[0] = "absent".into(),
            "overlap" => s.groups[0].members[0] = "a".into(),
            "duplicate-group" => s.groups[0].id = s.groups[1].id.clone(),
            "duplicate-member" => s.groups[0].members[0] = s.groups[0].members[1].clone(),
            _ => unreachable!(),
        }
        assert!(
            render_linked_html(&source, &ExplorerOptions::default(), &s).is_err(),
            "{mutation}"
        );
    }
    let mut changed = source.clone();
    changed.width += 1.0;
    assert!(render_linked_html(&changed, &ExplorerOptions::default(), &original).is_err());
}
#[test]
fn strict_spec_parser_rejects_duplicates_unknown_fields_versions_and_limits() {
    let source = scene();
    let value = serde_json::to_value(spec(&source)).unwrap();
    for field in [
        "format",
        "document_id",
        "scene_sha256",
        "instance_key",
        "groups",
    ] {
        let mut v = value.clone();
        v.as_object_mut().unwrap().remove(field);
        assert!(SelectionLinks::parse(&serde_json::to_vec(&v).unwrap()).is_err());
        let mut v = value.clone();
        v[field] = Value::Null;
        assert!(SelectionLinks::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let raw = serde_json::to_string(&value).unwrap();
    let dup = raw.replacen(
        "\"format\":",
        "\"format\":\"vizir-selection-links/1\",\"format\":",
        1,
    );
    assert!(SelectionLinks::parse(dup.as_bytes()).is_err());
    assert!(SelectionLinks::parse(&vec![b' '; vizir_web::MAX_LINK_SPEC_BYTES + 1]).is_err());
    let mut s = spec(&source);
    s.groups.clear();
    assert!(s.validate().is_err());
    let mut s = spec(&source);
    s.groups[0].members = vec!["a".into()];
    assert!(s.validate().is_err());
    let mut s = spec(&source);
    s.groups[0].members = (0..257).map(|n| format!("node-{n}")).collect();
    assert!(s.validate().is_err());
    for id in ["", "1bad", "Capital", "a\n", "a b", "__proto__"] {
        let mut s = spec(&source);
        s.groups[0].id = id.into();
        assert!(s.validate().is_err());
    }
    let mut s = spec(&source);
    s.groups = (0..17)
        .map(|g| SelectionGroup {
            id: format!("g-{g}"),
            members: (0..256).map(|i| format!("n-{g}-{i}")).collect(),
        })
        .collect();
    assert!(s.validate().is_err());
}
#[test]
fn v1_schema_and_capabilities_stay_narrow_and_svg_uses_shared_emission() {
    let source = scene();
    let v1 = render_html(&source, &ExplorerOptions::default()).unwrap();
    let v2 = render_linked_html(&source, &ExplorerOptions::default(), &spec(&source)).unwrap();
    let svg = |s: &str| s[s.find("<svg ").unwrap()..s.find("</svg>").unwrap() + 6].to_owned();
    assert_eq!(svg(&v1.html), svg(&v2.html));
    assert!(
        vizir_web::capabilities()
            .unsupported
            .contains("interaction.select.linked")
    );
    assert!(
        vizir_web::linked_capabilities()
            .supports
            .contains("interaction.select.linked-explicit")
    );
    assert!(
        vizir_web::interaction_schema()["properties"]
            .get("link_groups")
            .is_none()
    );
    assert_eq!(
        vizir_web::linked_interaction_schema()["properties"]["format"]["const"],
        vizir_web::LINKED_FORMAT
    );
}

#[test]
fn v2_bounds_qualified_reference_expansion_before_cloning_and_v1_stays_supported() {
    let mut source = scene();
    source.document_id = "d".repeat(4096);
    let template = source.nodes[1].clone();
    source.nodes = (0..1100)
        .map(|i| {
            let mut n = template.clone();
            if let SceneNode::Group { id, .. } = &mut n {
                *id = format!("n-{i}");
            }
            n
        })
        .collect();
    let v1 = render_html(&source, &ExplorerOptions::default()).unwrap();
    let links = SelectionLinks {
        format: vizir_web::LINKS_FORMAT.into(),
        document_id: source.document_id.clone(),
        scene_sha256: v1.manifest.scene_sha256.clone(),
        instance_key: "main".into(),
        groups: vec![SelectionGroup {
            id: "group".into(),
            members: vec!["n-0".into(), "n-1".into()],
        }],
    };
    let err = render_linked_html(&source, &ExplorerOptions::default(), &links)
        .unwrap_err()
        .to_string();
    assert!(err.contains("qualified link-reference expansion"), "{err}");
    let mut value: Value = serde_json::from_str(metadata(&v1.html)).unwrap();
    value["format"] = vizir_web::LINKED_FORMAT.into();
    value["profile"] = vizir_web::LINKED_PROFILE.into();
    value["link_groups"] = serde_json::to_value(&links.groups).unwrap();
    assert!(
        LinkedInteractionContext::parse(&serde_json::to_vec(&value).unwrap())
            .unwrap_err()
            .to_string()
            .contains("qualified link-reference expansion")
    );
}
