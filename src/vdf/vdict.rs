/// A value in a VDF file - either a string or a nested dictionary.
#[derive(Debug, Clone)]
pub enum VDFValue {
    String(String),
    Dict(VDFDict),
}

/// A dictionary of key-value pairs from a VDF file.
/// Keys can have duplicate entries, which is valid in VDF format.
#[derive(Debug, Clone, Default)]
pub struct VDFDict {
    entries: Vec<(String, VDFValue)>,
}

impl VDFDict {
    /// Create an empty VDFDict.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Insert a string value.
    pub fn insert(&mut self, key: String, value: String) {
        self.entries.push((key, VDFValue::String(value)));
    }

    /// Insert a nested dictionary.
    pub fn insert_dict(&mut self, key: String, value: VDFDict) {
        self.entries.push((key, VDFValue::Dict(value)));
    }

    /// Get the first string value for a key.
    pub fn get(&self, key: &str) -> Option<&str> {
        for (k, v) in &self.entries {
            if k == key {
                if let VDFValue::String(s) = v {
                    return Some(s);
                }
            }
        }
        None
    }

    /// Get the first dictionary value for a key.
    pub fn get_dict(&self, key: &str) -> Option<&VDFDict> {
        for (k, v) in &self.entries {
            if k == key {
                if let VDFValue::Dict(d) = v {
                    return Some(d);
                }
            }
        }
        None
    }

    /// Get all string values for a key (VDF allows duplicate keys).
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(k, _)| k == key)
            .filter_map(|(_, v)| {
                if let VDFValue::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all dictionary values for a key.
    pub fn get_all_dicts(&self, key: &str) -> Vec<&VDFDict> {
        self.entries
            .iter()
            .filter(|(k, _)| k == key)
            .filter_map(|(_, v)| {
                if let VDFValue::Dict(d) = v {
                    Some(d)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Iterate over all keys (may contain duplicates).
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Iterate over all key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &VDFValue)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Return the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if any keys appear more than once (recursively).
    pub fn has_duplicates(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        for (key, value) in &self.entries {
            if !seen.insert(key) {
                return true;
            }
            if let VDFValue::Dict(d) = value {
                if d.has_duplicates() {
                    return true;
                }
            }
        }
        false
    }
}
