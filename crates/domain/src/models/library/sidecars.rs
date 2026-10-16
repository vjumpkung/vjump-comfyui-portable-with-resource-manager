//! The exact same-stem sidecar suffixes of a model file (FR-MODEL-2).
use crate::shared::model_values::SafeFileName;

/// Normative and exhaustive: no `.metadata.json`, no wildcard matches.
pub const SIDECAR_SUFFIXES: [&str; 3] = [".preview.png", ".civitai.info", ".json"];

/// The name without its last extension (the whole name when there is none).
pub fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => name,
        Some(index) => &name[..index],
    }
}

/// The stem before a recognized suffix and that suffix, if the name ends with one.
pub fn sidecar_parts(name: &str) -> Option<(&str, &'static str)> {
    SIDECAR_SUFFIXES.iter().find_map(|suffix| {
        let cut = name.len().checked_sub(suffix.len())?;
        if cut == 0 || !name.is_char_boundary(cut) {
            return None;
        }
        name[cut..]
            .eq_ignore_ascii_case(suffix)
            .then(|| (&name[..cut], *suffix))
    })
}

/// The sidecar names of `model`, in suffix order (not checked for existence).
pub fn sidecar_names(model: &SafeFileName) -> Vec<SafeFileName> {
    let stem = stem_of(model.as_str());
    SIDECAR_SUFFIXES
        .iter()
        .filter_map(|suffix| SafeFileName::new(format!("{stem}{suffix}")).ok())
        .collect()
}

/// The matching sidecar name after the model was renamed to `new_model`.
pub fn renamed_sidecar(
    new_model: &SafeFileName,
    old_sidecar: &SafeFileName,
) -> Option<SafeFileName> {
    let (_, suffix) = sidecar_parts(old_sidecar.as_str())?;
    SafeFileName::new(format!("{}{suffix}", stem_of(new_model.as_str()))).ok()
}

fn same_stem(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// True when `name` is a recognized sidecar of another file in `siblings`. A `.json` without a
/// same-stem model beside it is an ordinary file.
pub fn is_sidecar_among<'a>(name: &str, siblings: impl IntoIterator<Item = &'a str>) -> bool {
    let Some((stem, _)) = sidecar_parts(name) else {
        return false;
    };
    siblings.into_iter().any(|other| {
        other != name && sidecar_parts(other).is_none() && same_stem(stem_of(other), stem)
    })
}

/// Other non-sidecar files that share the stem of `model` and therefore its sidecars.
pub fn ambiguous_stem_siblings<'a>(
    model: &str,
    siblings: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let stem = stem_of(model);
    siblings
        .into_iter()
        .filter(|other| {
            *other != model && sidecar_parts(other).is_none() && same_stem(stem_of(other), stem)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suffix_set_is_exactly_three() {
        assert_eq!(SIDECAR_SUFFIXES, [".preview.png", ".civitai.info", ".json"]);
        let model = SafeFileName::new("foo.safetensors").unwrap();
        let names: Vec<_> = sidecar_names(&model)
            .iter()
            .map(|n| n.as_str().to_owned())
            .collect();
        assert_eq!(names, ["foo.preview.png", "foo.civitai.info", "foo.json"]);
        assert!(sidecar_parts("foo.metadata.json").is_some_and(|(s, _)| s == "foo.metadata"));
    }

    #[test]
    fn a_lone_json_is_an_ordinary_file() {
        assert!(!is_sidecar_among(
            "config.json",
            ["config.json", "other.safetensors"]
        ));
        assert!(is_sidecar_among(
            "foo.json",
            ["foo.json", "foo.safetensors"]
        ));
        assert!(is_sidecar_among(
            "foo.preview.png",
            ["foo.preview.png", "foo.ckpt"]
        ));
        assert!(!is_sidecar_among(
            "foo.safetensors",
            ["foo.safetensors", "foo.json"]
        ));
    }

    #[test]
    fn shared_stems_are_ambiguous() {
        assert_eq!(
            ambiguous_stem_siblings(
                "foo.safetensors",
                ["foo.safetensors", "foo.ckpt", "foo.json"]
            ),
            ["foo.ckpt"]
        );
        assert!(
            ambiguous_stem_siblings("foo.safetensors", ["foo.safetensors", "bar.ckpt"]).is_empty()
        );
    }

    #[test]
    fn sidecar_rename_follows_the_new_stem() {
        let new = SafeFileName::new("bar.safetensors").unwrap();
        let sidecar = SafeFileName::new("foo.civitai.info").unwrap();
        assert_eq!(
            renamed_sidecar(&new, &sidecar).unwrap().as_str(),
            "bar.civitai.info"
        );
    }
}
