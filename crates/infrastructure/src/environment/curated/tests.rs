use std::collections::HashSet;

use super::*;

/// The reference installer's order with the commits resolved at 2026-10-16T17:11Z
/// (`docs/handoff/plans/m6-reference-data.md`): repository, branch and commit.
const REFERENCE: [(&str, &str, &str); 22] = [
    (
        "pollockjj/ComfyUI-MultiGPU",
        "main",
        "37a73feda5804045e4f39d8c640905d62a2169d1",
    ),
    (
        "molbal/ComfyUI-GGUF",
        "main",
        "f1d050fac00b0505cba519e44ad7fb18c34318b0",
    ),
    (
        "kijai/ComfyUI-KJNodes",
        "main",
        "17195fcfa8a05f601b766a0f09c9733284f47416",
    ),
    (
        "seitanism/ComfyUI-H3-Motion-Context-MultiRef",
        "main",
        "361624fb406b63eb6694442eac6c895fc1533a70",
    ),
    (
        "Kosinkadink/ComfyUI-VideoHelperSuite",
        "main",
        "4d907bee61e92c2e65af3bd6383a4e4d356126d1",
    ),
    (
        "rgthree/rgthree-comfy",
        "dev",
        "5f940ddfbb83ff9c667f32ae7d0c3ad0bb8cc9ee",
    ),
    (
        "crystian/ComfyUI-Crystools",
        "main",
        "2f18256c5b5063937106f29a8e0a7db3ae3869b7",
    ),
    (
        "kijai/ComfyUI-WanVideoWrapper",
        "main",
        "088128b224242e110d3906c6750e9a3a348a659b",
    ),
    (
        "pythongosssss/ComfyUI-Custom-Scripts",
        "main",
        "609f3afaa74b2f88ef9ce8d939626065e3247469",
    ),
    (
        "WASasquatch/was-node-suite-comfyui",
        "main",
        "49a1098630dedff9e0ef85e8e6c4d9a4ac85f8a1",
    ),
    (
        "1038lab/ComfyUI-QwenVL",
        "main",
        "1b67b443918801f571714bab636edc1845b7002a",
    ),
    (
        "kijai/ComfyUI-MelBandRoFormer",
        "main",
        "92c86854e6654f4aacc97484471af95c98ea16d4",
    ),
    (
        "ClownsharkBatwing/RES4LYF",
        "main",
        "56e44755d866c87ff9cb4b6608c8e42155db6ecc",
    ),
    (
        "kijai/ComfyUI-SolAttn_triton",
        "main",
        "26d816ebd4f1e43a2c6e4d4759be3137f10a7a73",
    ),
    (
        "vjumpkung/comfyui-infinitetalk-native-sampler",
        "main",
        "2bcb9e7e75348044b960d7759438c97e6b4ab289",
    ),
    (
        "vjumpkung/comfyui-mosaic-blur",
        "master",
        "ea6e4d21b81e74d1f7fbc2c18d037d23afdaf6d3",
    ),
    (
        "kijai/ComfyUI-WanAnimatePreprocess",
        "main",
        "0e0b6a2a555625acf4d4aefb780e27d06937132f",
    ),
    (
        "vjumpkung/comfyui-wan-animate-2-loop-sampler",
        "main",
        "64f9b89c1c478000de8ec2db464d16ef4ab2954e",
    ),
    (
        "vjumpkung/comfyui-scail-2-loop-sampler",
        "main",
        "569fa4a3c19e78a20a7dfa239955efd64fe77881",
    ),
    (
        "PlagueKind/ComfyUI-PlagueKind-Nodes",
        "main",
        "d58d006a4ea32c25c06499f2ff104f0852a045a6",
    ),
    (
        "Smirnov75/ComfyUI-mxToolkit",
        "main",
        "7f7a0e584f12078a1c589645d866ae96bad0cc35",
    ),
    (
        "matlowai/ComfyUI-MAINodes",
        "main",
        "f4868b4a08e8a504ce86db54a17961d399ffa2bc",
    ),
];

#[test]
fn curated_catalog_matches_reference_order_and_commits() {
    let nodes = BundledCuratedNodes::load()
        .expect("the bundled catalog must parse and validate")
        .list();
    assert_eq!(nodes.len(), 22);
    for (node, (repository, branch, commit)) in nodes.iter().zip(REFERENCE) {
        assert_eq!(
            node.repository,
            format!("https://github.com/{repository}.git")
        );
        assert_eq!(node.branch, branch, "{repository}");
        assert_eq!(node.commit, commit, "{repository}");
        assert_eq!(
            node.provenance,
            "reference installer order; commits resolved at 2026-10-16T17:11Z"
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
        assert_eq!(node.commit.len(), 40);
    }
    let ids: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
    assert_eq!(ids.len(), 22, "ids are unique");
    let branches: Vec<_> = nodes
        .iter()
        .filter(|node| node.branch != "main")
        .map(|node| (node.id.as_str(), node.branch.as_str()))
        .collect();
    assert_eq!(
        branches,
        [("rgthree-comfy", "dev"), ("comfyui-mosaic-blur", "master")]
    );
}

#[test]
fn a_malformed_catalog_is_refused() {
    let entry = |repository: &str, branch: &str, commit: &str| {
        format!(r#"{{"repository":"{repository}","branch":"{branch}","commit":"{commit}"}}"#)
    };
    let good = "0123456789abcdef0123456789abcdef01234567";
    let catalog = |schema: u32, entries: &[String]| {
        format!(
            r#"{{"schema_version":{schema},"provenance":"p","nodes":[{}]}}"#,
            entries.join(",")
        )
    };
    let ok = entry("owner/Name", "main", good);
    assert!(BundledCuratedNodes::parse(&catalog(1, std::slice::from_ref(&ok))).is_ok());
    let cases: [(String, CuratedError); 11] = [
        ("{".into(), CuratedError::Parse),
        (
            catalog(1, &[ok.replace("main", "m\\u0000")]),
            CuratedError::Branch,
        ),
        (catalog(2, std::slice::from_ref(&ok)), CuratedError::Schema),
        (catalog(1, &[]), CuratedError::Empty),
        (
            catalog(1, &[entry("owner/Name.git", "main", good)]),
            CuratedError::Repository,
        ),
        (
            catalog(1, &[entry("Name", "main", good)]),
            CuratedError::Repository,
        ),
        (
            catalog(1, &[entry("a/b/c", "main", good)]),
            CuratedError::Repository,
        ),
        (
            catalog(1, &[entry("owner/Name", "../x", good)]),
            CuratedError::Branch,
        ),
        (
            catalog(1, &[entry("owner/Name", "main", &good.to_uppercase())]),
            CuratedError::Commit,
        ),
        (
            catalog(1, &[entry("owner/Name", "main", "abc")]),
            CuratedError::Commit,
        ),
        (
            // The ids derive from the lower-cased names, so a case difference is a duplicate.
            catalog(1, &[ok.clone(), entry("other/name", "main", good)]),
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
fn bundled_catalog_marks_only_plaguekind_unavailable() {
    let nodes = BundledCuratedNodes::load()
        .expect("the bundled catalog must parse and validate")
        .list();
    let unavailable: Vec<_> = nodes
        .iter()
        .filter(|node| node.unavailable.is_some())
        .collect();
    assert_eq!(unavailable.len(), 1);
    let node = unavailable[0];
    assert_eq!(node.id, "comfyui-plaguekind-nodes");
    assert_eq!(node.commit, "d58d006a4ea32c25c06499f2ff104f0852a045a6");
    let reason = node.unavailable.as_deref().unwrap();
    assert!(reason.contains("__pycache__"), "{reason}");
    // The entry keeps its place in the catalog order.
    assert_eq!(
        nodes.iter().position(|candidate| candidate.id == node.id),
        Some(19)
    );
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.unavailable.is_none())
            .count(),
        21
    );
}

#[test]
fn catalog_rejects_blank_or_control_unavailable_reason() {
    let good = "0123456789abcdef0123456789abcdef01234567";
    let catalog = |reason: &str| {
        format!(
            r#"{{"schema_version":1,"provenance":"p","nodes":[{{"repository":"owner/Name","branch":"main","commit":"{good}","unavailable":{}}}]}}"#,
            serde_json::to_string(reason).unwrap()
        )
    };
    let ok = BundledCuratedNodes::parse(&catalog("needs a clean checkout")).unwrap();
    assert_eq!(
        ok.list()[0].unavailable.as_deref(),
        Some("needs a clean checkout")
    );
    // A missing reason and a null one both leave the entry available.
    let without = format!(
        r#"{{"schema_version":1,"provenance":"p","nodes":[{{"repository":"owner/Name","branch":"main","commit":"{good}","unavailable":null}}]}}"#
    );
    assert_eq!(
        BundledCuratedNodes::parse(&without).unwrap().list()[0].unavailable,
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
