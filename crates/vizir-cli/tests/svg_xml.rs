use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn source() -> Value {
    json!({"version":"0.2", "id":"xml-identities", "width":900, "height":600,
    "datasets":{"data":{"key":"id","rows":[{"id":"a","x":1,"y":2},{"id":"b","x":2,"y":3}]}},
    "views":[{"kind":"chart.scatter","id":"scatter","dataset":"data","frame":{"x":0,"y":0,"width":450,"height":400},"x":{"field":"x"},"y":{"field":"y"}},
        {"kind":"diagram.graph","id":"graph","frame":{"x":450,"y":0,"width":450,"height":400},"nodes":[{"id":"first","label":"First"},{"id":"second","label":"Second"}],"edges":[{"from":"first","to":"second","label":"edge"}]},
        {"kind":"geometry.scene","id":"geometry","frame":{"x":0,"y":400,"width":900,"height":200},"children":[{"type":"group","id":"nested","children":[{"type":"text","id":"label","x":40,"y":80,"text":"Text","font_size":20}]}]}
    ]})
}

fn assert_scene_identity(scene: &Value, svg: &str) {
    let xml = roxmltree::Document::parse(svg).unwrap();
    assert_eq!(
        xml.descendants()
            .find(|n| n.has_tag_name("title"))
            .unwrap()
            .text(),
        scene["document_id"].as_str()
    );
    let mut nodes: Vec<_> = scene["nodes"].as_array().unwrap().iter().collect();
    while let Some(node) = nodes.pop() {
        if let Some(children) = node["children"].as_array() {
            nodes.extend(children);
        }
        let id = node["id"].as_str().unwrap();
        let matching: Vec<_> = xml
            .descendants()
            .filter(|n| n.attribute("id") == Some(id))
            .collect();
        assert_eq!(matching.len(), 1, "parsed identity {id:?}");
        let element = matching[0];
        for (attr, field) in [
            ("data-hir-node", "hir_node"),
            ("data-mir-node", "mir_node"),
            ("data-generated-by", "generated_by"),
            ("data-key", "data_key"),
        ] {
            assert_eq!(
                element.attribute(attr),
                node["origin"][field].as_str(),
                "{id:?} {attr}"
            );
        }
        if let Some(lineage) = node["origin"]["data_lineage"].as_array() {
            let expected = lineage
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(element.attribute("data-lineage"), Some(expected.as_str()));
        }
        if node["type"] == "text" {
            assert_eq!(element.text(), node["text"].as_str());
        }
    }
}

#[test]
fn cli_xml_round_trips_scatter_graph_geometry_and_persisted_theme() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.json");
    let themed = temp.path().join("themed.json");
    let svg = temp.path().join("out.svg");
    let special = "汉字😺é e\u{301} /:~.#?%[] &<>\"' \t\n\r\r\n";
    let mut value = source();
    // These distinct keys used to alias after XML attribute normalization.
    value["datasets"]["data"]["rows"] = json!(
        ["a b", "a\tb", "a\nb", "a\rb", special]
            .iter()
            .enumerate()
            .map(|(i, key)| json!({"id":key,"x":i,"y":i + 1}))
            .collect::<Vec<_>>()
    );
    value["views"][0]["title"] = json!("Title &<>\t\n\r\r\n");
    value["views"][1]["nodes"][0]["label"] = json!(special);
    value["views"][1]["edges"][0]["label"] = json!(special);
    value["views"][2]["children"][0]["id"] = json!(format!("group {special}"));
    value["views"][2]["children"][0]["children"][0]["id"] = json!(special);
    value["views"][2]["children"][0]["children"][0]["text"] = json!(special);
    write(&input, &value);
    success(cli(&[
        "normalize",
        path(&input),
        "--theme",
        "azure-dark",
        "-o",
        path(&themed),
    ]));
    // MIR document IDs are opaque, unlike HIR's narrower authored identifiers.
    let mut persisted: Value = serde_json::from_slice(&fs::read(&themed).unwrap()).unwrap();
    persisted["mir"]["document_id"] = json!(format!("document {special}"));
    write(&themed, &persisted);
    for input in [&input, &themed] {
        let scene: Value = serde_json::from_str(&success(cli(&["lower", path(input)]))).unwrap();
        success(cli(&[
            "render",
            path(input),
            "--format",
            "svg",
            "-o",
            path(&svg),
        ]));
        let bytes = fs::read_to_string(&svg).unwrap();
        assert_scene_identity(&scene, &bytes);
        success(cli(&[
            "render",
            path(input),
            "--format",
            "svg",
            "-o",
            path(&svg),
        ]));
        assert_eq!(bytes.as_bytes(), fs::read(&svg).unwrap());
    }
}

#[test]
fn cli_illegal_xml_fails_before_any_svg_png_or_manifest_publication() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.json");
    let saved = temp.path().join("themed.json");
    let manifest = temp.path().join("manifest.json");
    for field in [
        "view_title",
        "data_key",
        "graph_label",
        "geometry_id",
        "geometry_text",
    ] {
        for cp in ['\0', '\u{1}', '\u{c}', '\u{1f}', '\u{fffe}', '\u{ffff}'] {
            let mut value = source();
            let bad = format!("private{cp}payload");
            match field {
                "view_title" => value["views"][0]["title"] = json!(bad),
                "data_key" => value["datasets"]["data"]["rows"][0]["id"] = json!(bad),
                "graph_label" => value["views"][1]["nodes"][0]["label"] = json!(bad),
                "geometry_id" => value["views"][2]["children"][0]["id"] = json!(bad),
                "geometry_text" => {
                    value["views"][2]["children"][0]["children"][0]["text"] = json!(bad)
                }
                _ => unreachable!(),
            }
            write(&input, &value);
            // HIR/MIR/Scene2D retain opaque data; this restriction is target-only.
            success(cli(&["lower", path(&input)]));
            success(cli(&[
                "normalize",
                path(&input),
                "--theme",
                "sage",
                "-o",
                path(&saved),
            ]));
            for source in [&input, &saved] {
                for format in ["svg", "png"] {
                    let output = temp.path().join(format!("output.{format}"));
                    for previous in [false, true] {
                        if previous {
                            fs::write(&output, b"previous artifact").unwrap();
                            fs::write(&manifest, b"previous manifest").unwrap();
                        }
                        // Empty PATH also ensures XML rejection is independent of rasterizers.
                        let result = Command::new(env!("CARGO_BIN_EXE_vizir"))
                            .args([
                                "render",
                                path(source),
                                "--format",
                                format,
                                "-o",
                                path(&output),
                                "--manifest",
                                path(&manifest),
                            ])
                            .env("PATH", "")
                            .output()
                            .unwrap();
                        assert!(!result.status.success());
                        let error = String::from_utf8_lossy(&result.stderr);
                        assert!(error.contains("VIZ-SVG-0001"), "{field}: {error}");
                        assert!(error.contains(&format!("U+{:04X}", u32::from(cp))));
                        assert!(!error.contains("private"));
                        assert!(error.len() < 220);
                        if previous {
                            assert_eq!(fs::read(&output).unwrap(), b"previous artifact");
                            assert_eq!(fs::read(&manifest).unwrap(), b"previous manifest");
                            fs::remove_file(&output).unwrap();
                            fs::remove_file(&manifest).unwrap();
                        } else {
                            assert!(!output.exists());
                            assert!(!manifest.exists());
                        }
                    }
                }
            }
        }
    }
}
