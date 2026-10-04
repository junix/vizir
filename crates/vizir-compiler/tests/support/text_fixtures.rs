use vizir_compiler::{CompilationContext, FontFace, FontResources, TextContext, TextFaces};
pub fn profile() -> (CompilationContext, FontResources) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/fonts/manifest.json")).unwrap();
    let bytes: [&[u8]; 3] = [
        include_bytes!("../fixtures/fonts/VizIRFixtureSC-Regular.otf"),
        include_bytes!("../fixtures/fonts/VizIRFixtureSC-Medium.otf"),
        include_bytes!("../fixtures/fonts/VizIRFixtureSC-Bold.otf"),
    ];
    let mut resources = FontResources::new();
    let mut faces = Vec::new();
    for (font, bytes) in manifest["fonts"].as_array().unwrap().iter().zip(bytes) {
        let hash = font["sha256"].as_str().unwrap();
        resources.insert(hash, bytes.to_vec()).unwrap();
        faces.push(FontFace::new(
            hash,
            0,
            font["weight"].as_u64().unwrap() as u16,
        ));
    }
    let text = TextContext::new(
        "zh-CN",
        TextFaces::new(faces.remove(0), faces.remove(0), faces.remove(0)),
    );
    (CompilationContext::new().with_text(text), resources)
}
