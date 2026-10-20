//! ComfyUI extra model paths: the reader parses `extra_model_paths.yaml`; `writer` manages the
//! one section the app owns.
use crate::environment::layout::{Layout, Tool, tool_environment};
use application::comfy_runtime::{ExtraModelPaths, ModelRoot, RuntimeError};
use async_trait::async_trait;
use serde_yaml_ng::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod writer;
pub use writer::APP_SECTION;

pub struct YamlExtraModelPaths;
#[async_trait]
impl ExtraModelPaths for YamlExtraModelPaths {
    async fn read(&self, root: &Path) -> Result<Vec<ModelRoot>, RuntimeError> {
        let layout = Layout::new(root);
        let path = layout.comfyui_dir().join("extra_model_paths.yaml");
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(invalid("the configuration could not be read")),
        };
        if text.len() > 1 << 20 {
            return Err(invalid("the configuration exceeds 1 MiB"));
        }
        let variables = tool_environment(&layout, Tool::Python)
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        parse(&text, &layout.comfyui_dir(), &variables)
    }
}
fn invalid(message: &str) -> RuntimeError {
    RuntimeError::Failed(format!("extra_model_paths.yaml: {message}"))
}

fn expand(input: &str, variables: &BTreeMap<String, String>) -> String {
    let mut text = input.to_owned();
    if text == "~" || text.starts_with("~/") || (cfg!(windows) && text.starts_with("~\\")) {
        let home = variables.get(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        if let Some(home) = home {
            text = format!("{home}{}", &text[1..]);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let (name, end) = if chars[i] == '$' {
            i += 1;
            if chars.get(i) == Some(&'{') {
                i += 1;
                let key_start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                if i == chars.len() {
                    out.extend(&chars[start..i]);
                    break;
                }
                (chars[key_start..i].iter().collect::<String>(), i + 1)
            } else {
                let key_start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                (chars[key_start..i].iter().collect(), i)
            }
        } else if cfg!(windows) && chars[i] == '%' {
            i += 1;
            let key_start = i;
            while i < chars.len() && chars[i] != '%' {
                i += 1;
            }
            if i == chars.len() {
                out.extend(&chars[start..i]);
                break;
            }
            (chars[key_start..i].iter().collect(), i + 1)
        } else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        match variables.get(&name) {
            Some(v) => out.push_str(v),
            None => out.extend(&chars[start..end]),
        }
        i = end;
    }
    out
}
fn joined(base: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
pub fn parse(
    text: &str,
    yaml_directory: &Path,
    variables: &BTreeMap<String, String>,
) -> Result<Vec<ModelRoot>, RuntimeError> {
    let mut doc: Value = serde_yaml_ng::from_str(text)
        .map_err(|_| invalid("malformed YAML; correct the file before continuing"))?;
    doc.apply_merge()
        .map_err(|_| invalid("invalid YAML merge"))?;
    if doc.is_null() {
        return Ok(Vec::new());
    }
    let sections = doc
        .as_mapping()
        .ok_or_else(|| invalid("expected a mapping of sections"))?;
    let mut roots = Vec::new();
    for (section, value) in sections {
        let section = section
            .as_str()
            .ok_or_else(|| invalid("section names must be strings"))?;
        if value.is_null() {
            continue;
        }
        let mapping = value
            .as_mapping()
            .ok_or_else(|| invalid("each section must be a mapping"))?;
        let base = match mapping.get(Value::String("base_path".into())) {
            None => yaml_directory.to_path_buf(),
            Some(v) => joined(
                yaml_directory,
                &expand(
                    v.as_str()
                        .ok_or_else(|| invalid("base_path must be a string"))?,
                    variables,
                ),
            ),
        };
        let is_default = match mapping.get(Value::String("is_default".into())) {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid("is_default must be a boolean"))?,
        };
        for (category, paths) in mapping {
            let category = category
                .as_str()
                .ok_or_else(|| invalid("category names must be strings"))?;
            if matches!(category, "base_path" | "is_default") {
                continue;
            }
            let paths = paths
                .as_str()
                .ok_or_else(|| invalid("category paths must be strings"))?;
            for path in paths.lines().map(str::trim).filter(|p| !p.is_empty()) {
                roots.push(ModelRoot {
                    section: section.into(),
                    category: category.into(),
                    path: joined(&base, path),
                    is_default,
                });
            }
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_loader_order_default_and_multiline_paths() {
        let dir = std::env::current_dir().unwrap();
        let text = "first:\n  base_path: '$CACHE/models'\n  is_default: true\n  checkpoints: |\n    ckpt\n    other\nempty: null\nnext:\n  loras: loras\n";
        let vars = BTreeMap::from([(
            "CACHE".into(),
            dir.join("cache").to_string_lossy().into_owned(),
        )]);
        let roots = parse(text, &dir, &vars).unwrap();
        assert_eq!(roots.len(), 3);
        assert_eq!(roots[0].path, dir.join("cache/models/ckpt"));
        assert!(roots[1].is_default);
        assert_eq!(roots[2].path, dir.join("loras"));
        assert!(parse("x: [a]", &dir, &vars).is_err());
        assert!(parse("x:\n  loras: 123", &dir, &vars).is_err());
    }
}
