#[path = "support/wrapping_fixtures.rs"]
mod fixtures;
#[path = "support/text_fixtures.rs"]
mod old_fixtures;
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;
use vizir_compiler::{
    CompilationContext, MaterializationLimits, TextLayoutContext, TextLayoutTarget, TextLimits,
    build_compiled_scene, compile_with_context, compile_with_context_with_limits,
    parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{Document, Revision, SceneNode, apply_scene_patch, diff_scene, find_scene_node};
fn doc(text: &str) -> Document {
    serde_json::from_value(json!({"version":"0.2","id":"wrap","width":700,"height":500,"views":[{"kind":"geometry.scene","id":"g","frame":{"x":10,"y":10,"width":680,"height":480},"children":[{"type":"text","id":"label","x":30,"y":60,"font_size":24,"anchor":"start","weight":"regular","text":text}]}]})).unwrap()
}
fn policy(width: f64, lines: u32, height: f64) -> TextLayoutContext {
    TextLayoutContext::new(vec![TextLayoutTarget::new(
        "g", "label", width, lines, height,
    )])
}
fn context() -> (CompilationContext, vizir_compiler::FontResources) {
    let (ctx, r) = fixtures::profile();
    (ctx.with_text_layout(policy(180., 10, 40.)), r)
}
fn mutate(d: &Document, f: impl FnOnce(&mut Value)) -> Document {
    let mut v = serde_json::to_value(d).unwrap();
    f(&mut v);
    serde_json::from_value(v).unwrap()
}
fn explanation(scene: &vizir_core::Scene2D) -> &str {
    let SceneNode::Path { origin, .. } = find_scene_node(&scene.nodes, "g/label").unwrap() else {
        panic!("not outlined")
    };
    &origin.explanation
}
fn ranges(scene: &vizir_core::Scene2D) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let text = explanation(scene)
        .split("source byte coverage [")
        .nth(1)
        .unwrap()
        .split("]; original text:")
        .next()
        .unwrap();
    text.split("; ")
        .map(|line| {
            let parts: Vec<_> = line.split(' ').collect();
            let range = |s: &str| {
                let p: Vec<usize> = s.split("..").map(|n| n.parse().unwrap()).collect();
                p[0]..p[1]
            };
            (range(parts[0]), range(parts[2]))
        })
        .collect()
}
#[test]
fn hardbreak_grammar_preserves_every_byte_blank_and_terminal_lines() {
    let (ctx, res) = context();
    for source in [
        "",
        "\n",
        "\n\n",
        "A\n\n",
        "\r\nA\r\n",
        "A\u{2028}\u{2029}",
        "  A  \n B ",
    ] {
        let result = compile_with_context(&doc(source), &ctx, &res).unwrap();
        let spans = ranges(&result.scene);
        let reconstructed: String = spans
            .iter()
            .map(|(s, b)| format!("{}{}", &source[s.clone()], &source[b.clone()]))
            .collect();
        assert_eq!(reconstructed, source);
        assert_eq!(
            serde_json::to_value(&result.mir.mir).unwrap()["views"][0]["children"][0]["text"],
            source
        );
        assert!(spans.last().unwrap().1.end == source.len());
        if source == "A\n\n" {
            assert_eq!(spans.len(), 3);
            assert_eq!(spans[2].0, 3..3);
        }
    }
}
#[test]
fn wrapping_covers_all_weights_ligatures_combining_and_cjk_punctuation() {
    let (ctx, res) = context();
    for source in [
        "AV office ffi Cafe\u{301} A\u{30a} x\u{301} gypq",
        "中文（字体测量），保留「标点」。中文测试！",
        "  中文  中文  中文  ",
        "A\u{30a}\u{301} A\u{30a}\u{301} A\u{30a}\u{301}",
    ] {
        for weight in ["regular", "medium", "bold"] {
            let d = mutate(&doc(source), |v| {
                v["views"][0]["children"][0]["weight"] = weight.into()
            });
            let c = compile_with_context(&d, &ctx, &res).unwrap();
            let boundaries: Vec<_> = source
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([source.len()])
                .collect();
            let spans = ranges(&c.scene);
            assert!(!spans.is_empty());
            for (text, _) in spans {
                assert!(boundaries.contains(&text.start));
                assert!(boundaries.contains(&text.end));
                let s = &source[text];
                assert!(!s.starts_with(['，', '。', '）', '」', '！']));
                assert!(!s.ends_with(['（', '「']));
            }
        }
    }
}
#[test]
fn whitespace_advance_and_unbreakable_segments_cannot_be_dropped() {
    let (mut ctx, res) = context();
    ctx.text_layout = Some(policy(25., 10, 40.));
    assert!(compile_with_context(&doc("A"), &ctx, &res).is_ok());
    for source in [
        "A     ",
        "     A",
        "officeoffice",
        "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}",
    ] {
        assert!(
            compile_with_context(&doc(source), &ctx, &res)
                .unwrap_err()
                .to_string()
                .contains("unbreakable"),
            "{source:?}"
        );
    }
}
#[test]
fn explicit_scope_rejects_missing_ambiguous_nontext_and_other_view_targets() {
    let (ctx, res) = context();
    let mut wrong = ctx.clone();
    wrong.text_layout.as_mut().unwrap().targets[0].node_id = "missing".into();
    assert!(
        compile_with_context(&doc("A"), &wrong, &res)
            .unwrap_err()
            .to_string()
            .contains("exactly one")
    );
    let rect =
        mutate(
            &doc("A"),
            |v| {
                v["views"][0]["children"][0] =
                    json!({"type":"rect","id":"label","x":0,"y":0,"width":10,"height":10})
            },
        );
    assert!(
        compile_with_context(&rect, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("non-text")
    );
    let duplicate = mutate(&doc("A"), |v| {
        let n = v["views"][0]["children"][0].clone();
        v["views"][0]["children"].as_array_mut().unwrap().push(n);
    });
    assert!(compile_with_context(&duplicate, &ctx, &res).is_err());
    let other = mutate(&doc("A"), |v| {
        v["views"][0]["children"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"text","id":"other","x":300,"y":60,"text":"A\nB","anchor":"start"}))
    });
    assert!(
        compile_with_context(&other, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("single-line")
    );
    let mut no_text = ctx.clone();
    no_text.text = None;
    assert!(compile_with_context(&doc("A"), &no_text, &res).is_err());
}
#[test]
fn no_policy_is_identical_and_newlines_remain_opt_in() {
    let (ctx, res) = old_fixtures::profile();
    let source = doc("Café Å中文");
    let a = compile_with_context(&source, &ctx, &res).unwrap();
    let mut null = serde_json::to_value(&a.mir).unwrap();
    null["context"]["text_layout"] = Value::Null;
    let parsed = parse_compiled_mir_json(&serde_json::to_vec(&null).unwrap()).unwrap();
    assert_eq!(a.mir, parsed);
    assert_eq!(a.scene, build_compiled_scene(&parsed, &res).unwrap());
    assert!(
        !serde_json::to_string(&a.mir)
            .unwrap()
            .contains("text_layout")
    );
    assert!(compile_with_context(&doc("A\nB"), &ctx, &res).is_err());
}
#[test]
fn replay_refresh_and_stable_id_patches_preserve_source() {
    let (ctx, res) = context();
    let a = compile_with_context(
        &doc("中文（字体测量），保留空白。\n\nAV office ffi"),
        &ctx,
        &res,
    )
    .unwrap();
    let decoded = parse_compiled_mir_json(&serde_json::to_vec(&a.mir).unwrap()).unwrap();
    assert_eq!(a.scene, build_compiled_scene(&decoded, &res).unwrap());
    assert_eq!(decoded, rematerialize_compiled_mir(&decoded, &res).unwrap());
    let b = compile_with_context(&doc("中文测试。\nAV office"), &ctx, &res).unwrap();
    let patch = diff_scene(&a.scene, &b.scene, Revision(1), Revision(2), "wrap").unwrap();
    assert_eq!(
        apply_scene_patch(&a.scene, Revision(1), &patch).unwrap().0,
        b.scene
    );
    let SceneNode::Path { id, origin, .. } = find_scene_node(&b.scene.nodes, "g/label").unwrap()
    else {
        panic!()
    };
    assert_eq!(id, "g/label");
    assert_eq!(origin.hir_node, "label");
}
#[test]
fn unsupported_controls_direction_and_missing_glyph_fail_usefully() {
    let (ctx, res) = context();
    for source in ["A\rB", "A\tB", "A\u{0085}B", "Aא", "A\u{202e}B", "A😀"] {
        let e = compile_with_context(&doc(source), &ctx, &res)
            .unwrap_err()
            .to_string();
        assert!(e.contains("VIZ-TEXT"), "{source:?}: {e}");
    }
}
#[test]
fn explicit_line_and_candidate_budgets_include_empty_work() {
    let (ctx, res) = context();
    for source in ["\n\n", "A A A A A A A A A A"] {
        let mut limits = TextLimits::new();
        limits.max_layout_lines = 1;
        assert!(
            compile_with_context_with_limits(
                &doc(source),
                &ctx,
                &res,
                MaterializationLimits::default(),
                limits
            )
            .unwrap_err()
            .to_string()
            .contains("line limit")
        );
    }
    let mut limits = TextLimits::new();
    limits.max_wrap_candidates = 0;
    assert!(
        compile_with_context_with_limits(
            &doc(""),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits
        )
        .unwrap_err()
        .to_string()
        .contains("candidate")
    );
    let mut limits = TextLimits::new();
    limits.max_text_bytes = 30;
    assert!(
        compile_with_context_with_limits(
            &doc("A A A A A A A A A A"),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits
        )
        .unwrap_err()
        .to_string()
        .contains("byte limit")
    );
}
#[test]
fn logical_empty_lines_and_real_ink_must_fit_view_and_canvas() {
    let (mut ctx, res) = context();
    ctx.text_layout = Some(policy(180., 2, 40.));
    assert!(
        compile_with_context(&doc("A\n\n"), &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("max_lines")
    );
    ctx.text_layout = Some(policy(180., 10, 40.));
    let d = mutate(&doc("\n\n"), |v| {
        v["views"][0]["children"][0]["y"] = 440.into()
    });
    assert!(
        compile_with_context(&d, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("overflows")
    );
    let d = mutate(&doc(""), |v| v["views"][0]["children"][0]["y"] = 1.into());
    assert!(
        compile_with_context(&d, &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("overflows")
    );
    ctx.text_layout = Some(policy(180., 10, 24.));
    assert!(
        compile_with_context(&doc("A\nB"), &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("ascent minus descent")
    );
    ctx.text_layout = Some(policy(180., 10, 40.));
    let d = doc(
        "A\u{30a}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\nA\u{30a}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
    );
    let stacked = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(ranges(&stacked.scene).len(), 2);
}
#[test]
fn source_slash_ids_and_nested_transforms_keep_one_path() {
    let (mut ctx, res) = context();
    ctx.text_layout.as_mut().unwrap().targets[0].view_id = "g/one".into();
    ctx.text_layout.as_mut().unwrap().targets[0].node_id = "a/b".into();
    let d = mutate(&doc("中文测试中文测试\nAV office"), |v| {
        v["views"][0]["id"] = "g/one".into();
        v["views"][0]["children"][0]["id"] = "a/b".into();
        let n = v["views"][0]["children"][0].take();
        v["views"][0]["children"][0] = json!({"type":"group","id":"nested","transform":{"translate":{"x":30.123456,"y":20.123456},"rotate_degrees":5.123456,"scale":{"x":0.9,"y":0.9}},"children":[n]});
    });
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert!(matches!(
        find_scene_node(&c.scene.nodes, "g/one/a/b"),
        Some(SceneNode::Path { .. })
    ));
    assert_eq!(c.scene, build_compiled_scene(&c.mir, &res).unwrap());
}

#[test]
fn exact_projected_width_boundary_and_all_anchors_include_overhang() {
    let (mut ctx, res) = context();
    for anchor in ["start", "middle", "end"] {
        let d = mutate(&doc("Å"), |v| {
            let n = &mut v["views"][0]["children"][0];
            n["anchor"] = anchor.into();
            n["weight"] = "bold".into();
            n["x"] = 300.into();
        });
        let c = compile_with_context(&d, &ctx, &res).unwrap();
        let advance: f64 = explanation(&c.scene)
            .split(" advance ")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let SceneNode::Path { bounds, .. } = find_scene_node(&c.scene.nodes, "g/label").unwrap()
        else {
            panic!()
        };
        let origin = 300.
            - match anchor {
                "middle" => advance / 2.,
                "end" => advance,
                _ => 0.,
            };
        let rounded = |n: f64| format!("{n:.4}").parse::<f64>().unwrap();
        let width = (bounds.x + bounds.width).max(rounded(origin + advance))
            - bounds.x.min(rounded(origin));
        ctx.text_layout = Some(policy(width + 0.0001, 10, 40.));
        assert!(compile_with_context(&d, &ctx, &res).is_ok());
        ctx.text_layout = Some(policy(width - 0.0001, 10, 40.));
        assert!(
            compile_with_context(&d, &ctx, &res)
                .unwrap_err()
                .to_string()
                .contains("unbreakable")
        );
        ctx.text_layout = Some(policy(180., 10, 40.));
    }
}

#[test]
fn maximum_empty_lines_and_minimum_size_are_real_bounded_operations() {
    let (mut ctx, res) = context();
    ctx.text_layout = Some(policy(1., 256, 0.4));
    let d = mutate(&doc(&"\n".repeat(255)), |v| {
        v["views"][0]["children"][0]["font_size"] = 0.25.into();
    });
    let c = compile_with_context(&d, &ctx, &res).unwrap();
    assert_eq!(ranges(&c.scene).len(), 256);
    let d = mutate(&doc(&"\n".repeat(256)), |v| {
        v["views"][0]["children"][0]["font_size"] = 0.25.into();
    });
    assert!(compile_with_context(&d, &ctx, &res).is_err());
    let d = mutate(&doc("A\nA"), |v| {
        v["views"][0]["children"][0]["font_size"] = 0.25.into();
    });
    assert_eq!(
        ranges(&compile_with_context(&d, &ctx, &res).unwrap().scene).len(),
        2
    );
}

#[test]
fn candidate_outline_cache_and_projection_are_metered() {
    let (ctx, res) = context();
    for field in ["glyphs", "outline", "cache"] {
        let mut limits = TextLimits::new();
        match field {
            "glyphs" => limits.max_glyphs = 1,
            "outline" => limits.max_outline_commands = 5,
            _ => limits.max_cache_bytes = 1,
        }
        let e = compile_with_context_with_limits(
            &doc("A A A A"),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("VIZ-TEXT-0003"), "{field}: {e}");
    }
    let mut limits = TextLimits::new();
    limits.max_wrap_candidates = 1;
    assert!(
        compile_with_context_with_limits(
            &doc("A A A A"),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits
        )
        .unwrap_err()
        .to_string()
        .contains("candidate")
    );
}

#[test]
fn soft_hyphen_rejection_reports_original_utf8_offset() {
    let (ctx, res) = context();
    let e = compile_with_context(&doc("中文\u{00ad}测试"), &ctx, &res)
        .unwrap_err()
        .to_string();
    assert!(e.contains("soft hyphen U+00AD at UTF-8 byte 6"), "{e}");
}
