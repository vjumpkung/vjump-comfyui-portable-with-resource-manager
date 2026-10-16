use std::collections::BTreeMap;

use super::error::EnvironmentError;

/// A normalized Python distribution name (PEP 503): lowercase, runs of `-`, `_` and `.`
/// collapsed to a single `-`. Wildcards such as `triton*` are not valid names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DistName(String);

impl DistName {
    pub fn new(raw: &str) -> Result<Self, EnvironmentError> {
        let mut normalized = String::with_capacity(raw.len());
        let mut previous_separator = false;
        for character in raw.chars() {
            if matches!(character, '-' | '_' | '.') {
                if !previous_separator {
                    normalized.push('-');
                }
                previous_separator = true;
            } else if character.is_ascii_alphanumeric() {
                normalized.push(character.to_ascii_lowercase());
                previous_separator = false;
            } else {
                return Err(EnvironmentError::InvalidDistributionName);
            }
        }
        let valid = !normalized.is_empty()
            && normalized.len() <= 100
            && !normalized.starts_with('-')
            && !normalized.ends_with('-');
        if valid {
            Ok(Self(normalized))
        } else {
            Err(EnvironmentError::InvalidDistributionName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DistName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One protected distribution whose installed version differs between two pin sets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PinChange {
    pub name: DistName,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The exact installed versions of the protected distributions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinSet(BTreeMap<DistName, String>);

impl PinSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: DistName, version: impl Into<String>) {
        self.0.insert(name, version.into());
    }

    pub fn get(&self, name: &DistName) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    pub fn contains(&self, name: &DistName) -> bool {
        self.0.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DistName, &str)> {
        self.0
            .iter()
            .map(|(name, version)| (name, version.as_str()))
    }

    /// Picks the protected distributions out of a full installed-package list.
    pub fn from_installed<'a>(
        installed: impl IntoIterator<Item = (&'a DistName, &'a str)>,
        protected: &[DistName],
    ) -> Self {
        let mut pins = Self::new();
        for (name, version) in installed {
            if protected.contains(name) {
                pins.insert(name.clone(), version);
            }
        }
        pins
    }

    /// The pins without the named distributions.
    pub fn without(&self, names: &[DistName]) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(name, _)| !names.contains(name))
                .map(|(name, version)| (name.clone(), version.clone()))
                .collect(),
        )
    }

    /// Every protected distribution that differs between `self` (before) and `after`.
    pub fn diff(&self, after: &Self) -> Vec<PinChange> {
        let mut names: Vec<&DistName> = self.0.keys().chain(after.0.keys()).collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .filter_map(|name| {
                let from = self.0.get(name);
                let to = after.0.get(name);
                (from != to).then(|| PinChange {
                    name: name.clone(),
                    from: from.cloned(),
                    to: to.cloned(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> DistName {
        DistName::new(value).unwrap()
    }

    #[test]
    fn names_are_normalized_and_wildcards_rejected() {
        assert_eq!(name("Triton_Windows").as_str(), "triton-windows");
        assert_eq!(name("a.b__c--d").as_str(), "a-b-c-d");
        for bad in ["", "triton*", "-x", "x-", "a b", "torch>=2"] {
            assert!(DistName::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn diff_reports_added_removed_and_changed_pins() {
        let mut before = PinSet::new();
        before.insert(name("torch"), "2.14.1+cu130");
        before.insert(name("torchvision"), "0.29.1+cu130");
        let mut after = PinSet::new();
        after.insert(name("torch"), "2.15.0");
        after.insert(name("triton"), "3.8.0");
        let changes = before.diff(&after);
        assert_eq!(
            changes,
            vec![
                PinChange {
                    name: name("torch"),
                    from: Some("2.14.1+cu130".into()),
                    to: Some("2.15.0".into())
                },
                PinChange {
                    name: name("torchvision"),
                    from: Some("0.29.1+cu130".into()),
                    to: None
                },
                PinChange {
                    name: name("triton"),
                    from: None,
                    to: Some("3.8.0".into())
                },
            ]
        );
        assert!(before.diff(&before).is_empty());
    }

    #[test]
    fn from_installed_keeps_only_protected_names() {
        let protected = [name("torch"), name("triton")];
        let installed = [
            (name("torch"), "2.14.1"),
            (name("numpy"), "2.2.0"),
            (name("triton"), "3.8.0"),
        ];
        let pins = PinSet::from_installed(
            installed.iter().map(|(name, version)| (name, *version)),
            &protected,
        );
        assert_eq!(pins.len(), 2);
        assert_eq!(pins.get(&name("torch")), Some("2.14.1"));
        assert!(!pins.contains(&name("numpy")));
        assert_eq!(pins.without(&[name("torch")]).len(), 1);
    }
}
