use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid model filename, relative path, hash, or identifier")]
pub struct ModelValueError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeFileName(String);
impl SafeFileName {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelValueError> {
        let value = value.into();
        let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|s| {
                matches!(
                    s,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if value.is_empty()
            || value.len() > 255
            || value.contains("..")
            || value.ends_with(['.', ' '])
            || reserved
            || value
                .chars()
                .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
        {
            return Err(ModelValueError);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn with_suffix(&self, n: u32) -> Result<Self, ModelValueError> {
        let (stem, ext) = self
            .0
            .rsplit_once('.')
            .filter(|(s, _)| !s.is_empty())
            .map_or((self.0.as_str(), String::new()), |(s, e)| {
                (s, format!(".{e}"))
            });
        Self::new(format!("{stem} ({n}){ext}"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RelativeModelPath(Vec<SafeFileName>);
impl RelativeModelPath {
    pub fn new(value: &str) -> Result<Self, ModelValueError> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        if value.contains('\\') {
            return Err(ModelValueError);
        }
        value
            .split('/')
            .map(SafeFileName::new)
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
    pub fn components(&self) -> &[SafeFileName] {
        &self.0
    }
    pub fn as_string(&self) -> String {
        self.0
            .iter()
            .map(SafeFileName::as_str)
            .collect::<Vec<_>>()
            .join("/")
    }
    pub fn join(&self, name: SafeFileName) -> Self {
        let mut parts = self.0.clone();
        parts.push(name);
        Self(parts)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256(String);
impl Sha256 {
    pub fn new(value: &str) -> Result<Self, ModelValueError> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ModelValueError);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(String);
impl JobId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelValueError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 100
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ModelValueError);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_names_and_paths() {
        for s in [
            "",
            "..",
            "a..b",
            "NUL.pt",
            "com1.bin",
            "LPT².txt",
            "a/hi",
            "a\\hi",
            "a:b",
            "a.",
            "a ",
            "a\0b",
            "x?y",
        ] {
            assert!(SafeFileName::new(s).is_err(), "{s:?}");
        }
        assert!(SafeFileName::new("โมเดล.safetensors").is_ok());
        assert!(RelativeModelPath::new("/tmp").is_err());
        assert_eq!(
            SafeFileName::new("a.pt")
                .unwrap()
                .with_suffix(2)
                .unwrap()
                .as_str(),
            "a (2).pt"
        );
    }
}
