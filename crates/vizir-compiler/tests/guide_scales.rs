use std::collections::BTreeMap;
use std::path::PathBuf;

use vizir_compiler::{build_scene, compile};
use vizir_core::{
    ChartMark, Color, GuideKind, GuideOrient, MirChart, MirScale, MirView, Scene2D, SceneNode,
    View, VizMir, find_scene_node, parse_document, validate_mir,
};

fn example(name: &str) -> VizMir {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let mut mir = compile(&parse_document(path).unwrap()).unwrap().mir;
    // Legacy line/bar MIR has an implicit legend; create explicit references
    // here to exercise the public MIR consumer without changing canonical HIR.
    for view in &mut mir.views {
        if let MirView::Chart(chart) = view
            && !chart
                .guides
                .iter()
                .any(|guide| guide.kind == GuideKind::Legend)
            && let Some(scale) = chart
                .scales
                .iter()
                .find(|scale| matches!(scale, MirScale::OrdinalColor { .. }))
        {
            let mut guide = chart.guides[0].clone();
            guide.id = format!("{}/guides/color-legend", chart.id);
            guide.kind = GuideKind::Legend;
            guide.orient = GuideOrient::Right;
            guide.scale = scale.id().to_owned();
            guide.label = "series".to_owned();
            chart.guides.push(guide);
        }
    }
    mir
}

fn first_chart(mir: &mut VizMir) -> &mut MirChart {
    let MirView::Chart(chart) = &mut mir.views[0] else {
        panic!("chart")
    };
    chart
}

fn text<'a>(scene: &'a Scene2D, id: &str) -> &'a str {
    let Some(SceneNode::Text { text, .. }) = find_scene_node(&scene.nodes, id) else {
        panic!("missing text {id}")
    };
    text
}

fn rename_scales(chart: &mut MirChart) {
    let renamed = chart
        .scales
        .iter()
        .enumerate()
        .map(|(index, scale)| (scale.id().to_owned(), format!("opaque-scale-{index}")))
        .collect::<BTreeMap<_, _>>();
    for scale in &mut chart.scales {
        let id = match scale {
            MirScale::Linear { id, .. }
            | MirScale::Band { id, .. }
            | MirScale::OrdinalColor { id, .. }
            | MirScale::QuantizeColor { id, .. } => id,
        };
        *id = renamed[id].clone();
    }
    for guide in &mut chart.guides {
        guide.scale = renamed[&guide.scale].clone();
    }
    let bindings = match &mut chart.mark {
        ChartMark::Heatmap { .. } => unreachable!("heatmap guide IDs have a separate test matrix"),
        ChartMark::Symbol { x, y, color, .. }
        | ChartMark::Line { x, y, color, .. }
        | ChartMark::Area { x, y, color, .. } => (x, y, color),
        ChartMark::Bar {
            category,
            value,
            color,
            ..
        } => (category, value, color),
    };
    bindings.0.scale = renamed[&bindings.0.scale].clone();
    bindings.1.scale = renamed[&bindings.1.scale].clone();
    if let Some(color) = bindings.2 {
        color.scale = renamed[&color.scale].clone();
    }
    chart.scales.reverse();
}

#[test]
fn opaque_scale_ids_preserve_all_scatter_line_and_bar_guides() {
    for source in [
        "chart/service-health.viz.yaml",
        "chart/incident-latency.viz.yaml",
        "chart/model-evaluation.viz.yaml",
    ] {
        let mut mir = example(source);
        let expected = build_scene(&mir).unwrap();
        for view in &mut mir.views {
            if let MirView::Chart(chart) = view {
                rename_scales(chart);
            }
        }
        validate_mir(&mir).unwrap();
        let actual = build_scene(&mir).unwrap();
        for node in &expected.nodes {
            let SceneNode::Group { children, .. } = node else {
                unreachable!()
            };
            for child in children.iter().filter(|node| {
                node.id().contains("/axis/")
                    || node.id().contains("/grid/")
                    || node.id().contains("/legend/")
            }) {
                assert_eq!(
                    find_scene_node(&actual.nodes, child.id()),
                    Some(child),
                    "{source}: {}",
                    child.id()
                );
            }
        }
    }
}

#[test]
fn axis_guides_use_their_own_domains_without_changing_marks() {
    let mut mir = example("chart/incident-latency.viz.yaml");
    let expected = build_scene(&mir).unwrap();
    let chart = first_chart(&mut mir);
    for (index, domain) in [(0, [1000.0, 2000.0]), (1, [-500.0, -100.0])] {
        let mut scale = chart.scales[index].clone();
        let MirScale::Linear {
            id, domain: values, ..
        } = &mut scale
        else {
            unreachable!()
        };
        *id = format!("distinct-{index}");
        *values = domain;
        chart.guides[index].scale = id.clone();
        chart.scales.push(scale);
    }
    validate_mir(&mir).unwrap();
    let actual = build_scene(&mir).unwrap();
    for (id, expected) in [
        ("recovery-curve/axis/x/label/0", "1.0k"),
        ("recovery-curve/axis/x/label/5", "2.0k"),
        ("recovery-curve/axis/y/label/0", "-100"),
        ("recovery-curve/axis/y/label/5", "-500"),
    ] {
        assert_eq!(text(&actual, id), expected);
    }
    let SceneNode::Group { children, .. } = &expected.nodes[0] else {
        unreachable!()
    };
    for node in children
        .iter()
        .filter(|node| node.id().contains("/series/"))
    {
        assert_eq!(find_scene_node(&actual.nodes, node.id()), Some(node));
    }
}

#[test]
fn band_guide_uses_its_own_domain_and_ignores_suffix_decoys() {
    let mut mir = example("chart/model-evaluation.viz.yaml");
    let chart = first_chart(&mut mir);
    let mut scale = chart.scales[0].clone();
    let MirScale::Band { id, domain, .. } = &mut scale else {
        unreachable!()
    };
    *id = "display-categories".to_owned();
    domain.reverse();
    let first = domain[0].clone();
    chart.guides[0].scale = id.clone();
    chart.scales.push(scale);
    validate_mir(&mir).unwrap();
    let scene = build_scene(&mir).unwrap();
    assert_eq!(text(&scene, "benchmark-quality/axis/x/category/0"), first);
}

#[test]
fn legend_guide_uses_its_own_scale_for_layout_labels_and_swatches() {
    let mut mir = example("chart/incident-latency.viz.yaml");
    let expected = build_scene(&mir).unwrap();
    let chart = first_chart(&mut mir);
    let mut scale = chart
        .scales
        .iter()
        .find(|scale| matches!(scale, MirScale::OrdinalColor { .. }))
        .unwrap()
        .clone();
    let MirScale::OrdinalColor { id, domain, range } = &mut scale else {
        unreachable!()
    };
    *id = "display-colors".to_owned();
    domain.reverse();
    let first = domain[0].clone();
    *range = vec![Color::hex("#FF00FF"); domain.len()];
    let guide = chart
        .guides
        .iter_mut()
        .find(|guide| guide.kind == GuideKind::Legend)
        .unwrap();
    guide.id = "explicit-legend".to_owned();
    guide.scale = id.clone();
    chart.scales.push(scale);
    validate_mir(&mir).unwrap();
    let scene = build_scene(&mir).unwrap();
    assert_eq!(text(&scene, "recovery-curve/legend/0/label"), first);
    let Some(SceneNode::Circle { style, origin, .. }) =
        find_scene_node(&scene.nodes, "recovery-curve/legend/0/swatch")
    else {
        unreachable!()
    };
    assert_eq!(style.fill, Color::hex("#FF00FF"));
    assert_eq!(origin.mir_node, "explicit-legend");
    let SceneNode::Group { children, .. } = &expected.nodes[0] else {
        unreachable!()
    };
    for node in children
        .iter()
        .filter(|node| node.id().contains("/series/"))
    {
        assert_eq!(find_scene_node(&scene.nodes, node.id()), Some(node));
    }
    // A distinct legend still owns its header reservation; it cannot draw over the plot.
    let chart = first_chart(&mut mir);
    let MirScale::OrdinalColor { domain, .. } = chart.scales.last_mut().unwrap() else {
        unreachable!()
    };
    domain[0] = "W".repeat(200);
    assert!(
        build_scene(&mir)
            .unwrap_err()
            .to_string()
            .contains("VIZ-LAYOUT-0004")
    );
}

#[test]
fn missing_and_wrong_kind_guide_references_are_diagnosed() {
    for (index, target, code) in [
        (0, "absent", "VIZ-RESOLVE-0006"),
        (2, "absent", "VIZ-RESOLVE-0006"),
        (0, "recovery-curve/color", "VIZ-TYPE-0203"),
        (2, "recovery-curve/x", "VIZ-TYPE-0203"),
    ] {
        let mut mir = example("chart/incident-latency.viz.yaml");
        first_chart(&mut mir).guides[index].scale = target.to_owned();
        let diagnostics = validate_mir(&mir).unwrap_err();
        assert!(diagnostics.iter().any(|error| error.code == code
            && error.source.as_deref() == Some(&format!("views[0].guides[{index}].scale"))));
        let error = build_scene(&mir).unwrap_err().to_string();
        assert!(error.contains(code) && error.contains(target), "{error}");
    }
}

#[test]
fn incompatible_guide_ranges_keep_the_header_layout_guard() {
    for index in 0..2 {
        let mut mir = example("chart/incident-latency.viz.yaml");
        let chart = first_chart(&mut mir);
        let mut scale = chart.scales[index].clone();
        let MirScale::Linear { id, range, .. } = &mut scale else {
            unreachable!()
        };
        *id = "misaligned-guide".to_owned();
        range[0] += 0.000001;
        chart.guides[index].scale = id.clone();
        chart.scales.push(scale);
        validate_mir(&mir).unwrap();
        assert!(
            build_scene(&mir)
                .unwrap_err()
                .to_string()
                .contains("VIZ-LAYOUT-0005")
        );
    }
}

#[test]
fn unsupported_or_duplicate_guide_slots_are_explicit_errors() {
    for case in [
        "right-axis",
        "bottom-legend",
        "left-band",
        "duplicate-axis",
        "duplicate-legend",
    ] {
        let mut mir = example("chart/model-evaluation.viz.yaml");
        let chart = first_chart(&mut mir);
        match case {
            "right-axis" => chart.guides[1].orient = GuideOrient::Right,
            "bottom-legend" => chart.guides[2].orient = GuideOrient::Bottom,
            "left-band" => chart.guides[1].scale = chart.guides[0].scale.clone(),
            _ => {
                let mut guide = chart.guides[if case == "duplicate-axis" { 0 } else { 2 }].clone();
                guide.id.push_str("/duplicate");
                chart.guides.push(guide);
            }
        }
        validate_mir(&mir).unwrap();
        assert!(
            build_scene(&mir)
                .unwrap_err()
                .to_string()
                .contains("VIZ-SCENE-0004"),
            "{case}"
        );
    }
}

#[test]
fn absent_guides_do_not_fall_back_to_mark_scale_names() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chart/incident-latency.viz.yaml");
    let mut document = parse_document(path).unwrap();
    let View::Line(chart) = &mut document.views[0] else {
        unreachable!()
    };
    chart.title = None;
    chart.series = None;
    let mut mir = compile(&document).unwrap().mir;
    first_chart(&mut mir).guides.clear();
    validate_mir(&mir).unwrap();
    let scene = build_scene(&mir).unwrap();
    let SceneNode::Group { children, .. } = &scene.nodes[0] else {
        unreachable!()
    };
    assert!(!children.iter().any(|node| node.id().contains("/axis/")
        || node.id().contains("/grid/")
        || node.id().contains("/legend/")));
}

#[test]
fn legacy_line_and_bar_implicit_legends_match_explicit_guides() {
    for source in [
        "chart/incident-latency.viz.yaml",
        "chart/model-evaluation.viz.yaml",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(source);
        let result = compile(&parse_document(path).unwrap()).unwrap();
        let MirView::Chart(chart) = &result.mir.views[0] else {
            unreachable!()
        };
        assert!(
            !chart
                .guides
                .iter()
                .any(|guide| guide.kind == GuideKind::Legend)
        );
        assert_eq!(result.scene, build_scene(&example(source)).unwrap());
    }
}

#[test]
fn fractional_round_tripped_mir_keeps_opaque_guide_references() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chart/service-health.viz.yaml");
    let base = parse_document(path).unwrap();
    for index in 1..=1000 {
        let mut document = base.clone();
        for view in &mut document.views {
            let frame = match view {
                View::Scatter(chart) => &mut chart.frame,
                View::Bar(chart) => &mut chart.frame,
                _ => unreachable!(),
            };
            frame.x = index as f64 / 13.0;
            frame.y = index as f64 / 17.0;
            frame.width = 760.0 + index as f64 / 19.0;
            frame.height = 720.0 + index as f64 / 23.0;
        }
        let mut mir = compile(&document).unwrap().mir;
        for view in &mut mir.views {
            let MirView::Chart(chart) = view else {
                unreachable!()
            };
            rename_scales(chart);
        }
        let mir = serde_json::from_slice(&serde_json::to_vec(&mir).unwrap()).unwrap();
        validate_mir(&mir).unwrap();
        let scene = build_scene(&mir).unwrap();
        assert!(find_scene_node(&scene.nodes, "latency-risk/axis/x/label/0").is_some());
        assert!(find_scene_node(&scene.nodes, "availability-ranking/axis/x/category/0").is_some());
    }
}
