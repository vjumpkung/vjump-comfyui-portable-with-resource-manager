use std::collections::HashSet;

use super::*;

/// The reference installer's order (`docs/handoff/plans/m6-reference-data.md`); every entry
/// installs the latest commit of its repository's default branch.
const REFERENCE: [&str; 22] = [
    "pollockjj/ComfyUI-MultiGPU",
    "molbal/ComfyUI-GGUF",
    "kijai/ComfyUI-KJNodes",
    "seitanism/ComfyUI-H3-Motion-Context-MultiRef",
    "Kosinkadink/ComfyUI-VideoHelperSuite",
    "rgthree/rgthree-comfy",
    "crystian/ComfyUI-Crystools",
    "kijai/ComfyUI-WanVideoWrapper",
    "pythongosssss/ComfyUI-Custom-Scripts",
    "WASasquatch/was-node-suite-comfyui",
    "1038lab/ComfyUI-QwenVL",
    "kijai/ComfyUI-MelBandRoFormer",
    "ClownsharkBatwing/RES4LYF",
    "kijai/ComfyUI-SolAttn_triton",
    "vjumpkung/comfyui-infinitetalk-native-sampler",
    "vjumpkung/comfyui-mosaic-blur",
    "kijai/ComfyUI-WanAnimatePreprocess",
    "vjumpkung/comfyui-wan-animate-2-loop-sampler",
    "vjumpkung/comfyui-scail-2-loop-sampler",
    "PlagueKind/ComfyUI-PlagueKind-Nodes",
    "Smirnov75/ComfyUI-mxToolkit",
    "matlowai/ComfyUI-MAINodes",
];

#[test]
fn curated_catalog_matches_reference_order() {
    let nodes = BundledCuratedNodes::load()
        .expect("the bundled catalog must parse and validate")
        .list();
    assert_eq!(nodes.len(), 22);
    for (node, repository) in nodes.iter().zip(REFERENCE) {
        assert_eq!(
            node.repository,
            format!("https://github.com/{repository}.git")
        );
        assert_eq!(
            node.provenance,
            "reference installer order; each entry installs the latest commit of its repository's default branch at setup time (owner decision 2026-10-20)"
        );
        // The id and the folder name derive from the repository name.
        let name = repository.rsplit('/').next().unwrap();
        assert_eq!(node.name, name);
        assert_eq!(node.id, name.to_ascii_lowercase());
        assert!(
            node.id.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.')),
            "{}",
            node.id
        );
        // Every entry is a well-formed HTTPS remote that Git can be given.
        let remote = validate_remote(&node.repository, false).expect("a valid HTTPS remote");
        assert_eq!(remote.identity, format!("https://github.com/{repository}"));
    }
    let ids: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
    assert_eq!(ids.len(), 22, "ids are unique");
}

#[test]
fn a_malformed_catalog_is_refused() {
    let entry = |repository: &str| format!(r#"{{"repository":"{repository}"}}"#);
    let catalog = |schema: u32, entries: &[String]| {
        format!(
            r#"{{"schema_version":{schema},"provenance":"p","nodes":[{}]}}"#,
            entries.join(",")
        )
    };
    let ok = entry("owner/Name");
    assert!(BundledCuratedNodes::parse(&catalog(2, std::slice::from_ref(&ok))).is_ok());
    let cases: [(String, CuratedError); 11] = [
        ("{".into(), CuratedError::Parse),
        (catalog(1, std::slice::from_ref(&ok)), CuratedError::Schema),
        (catalog(3, std::slice::from_ref(&ok)), CuratedError::Schema),
        (catalog(2, &[]), CuratedError::Empty),
        (
            catalog(2, &[entry("owner/Name.git")]),
            CuratedError::Repository,
        ),
        (catalog(2, &[entry("Name")]), CuratedError::Repository),
        (catalog(2, &[entry("a/b/c")]), CuratedError::Repository),
        (
            // A pinned commit is no longer part of the schema.
            catalog(
                2,
                &[r#"{"repository":"owner/Name","commit":"0123456789abcdef0123456789abcdef01234567"}"#
                    .into()],
            ),
            CuratedError::Parse,
        ),
        (
            // Neither is a branch.
            catalog(2, &[r#"{"repository":"owner/Name","branch":"main"}"#.into()]),
            CuratedError::Parse,
        ),
        (
            catalog(2, &[r#"{"branch":"main"}"#.into()]),
            CuratedError::Parse,
        ),
        (
            // The ids derive from the lower-cased names, so a case difference is a duplicate.
            catalog(2, &[ok.clone(), entry("other/name")]),
            CuratedError::Duplicate,
        ),
    ];
    for (json, expected) in cases {
        assert_eq!(
            BundledCuratedNodes::parse(&json).unwrap_err(),
            expected,
            "{json}"
        );
    }
}

#[test]
fn bundled_catalog_has_no_unavailable_entries() {
    let nodes = BundledCuratedNodes::load()
        .expect("the bundled catalog must parse and validate")
        .list();
    assert_eq!(nodes.len(), 22);
    assert!(nodes.iter().all(|node| node.unavailable.is_none()));
    // The entry keeps its place in the catalog order.
    assert_eq!(
        nodes
            .iter()
            .position(|node| node.id == "comfyui-plaguekind-nodes"),
        Some(19)
    );
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.unavailable.is_none())
            .count(),
        22
    );
}

#[test]
fn catalog_rejects_blank_or_control_unavailable_reason() {
    let catalog = |reason: &str| {
        format!(
            r#"{{"schema_version":2,"provenance":"p","nodes":[{{"repository":"owner/Name","unavailable":{}}}]}}"#,
            serde_json::to_string(reason).unwrap()
        )
    };
    let ok = BundledCuratedNodes::parse(&catalog("needs a clean checkout")).unwrap();
    assert_eq!(
        ok.list()[0].unavailable.as_deref(),
        Some("needs a clean checkout")
    );
    // A missing reason and a null one both leave the entry available.
    let without = r#"{"schema_version":2,"provenance":"p","nodes":[{"repository":"owner/Name","unavailable":null}]}"#;
    assert_eq!(
        BundledCuratedNodes::parse(without).unwrap().list()[0].unavailable,
        None
    );
    let too_long = "x".repeat(301);
    for reason in [
        "",
        "   ",
        " padded",
        "padded ",
        "line\nbreak",
        "bell\u{7}",
        "escape\u{1b}[0m",
        "nul\u{0}",
        too_long.as_str(),
    ] {
        assert_eq!(
            BundledCuratedNodes::parse(&catalog(reason)).unwrap_err(),
            CuratedError::Unavailable,
            "{reason:?}"
        );
    }
    assert!(BundledCuratedNodes::parse(&catalog(&"x".repeat(300))).is_ok());
}
