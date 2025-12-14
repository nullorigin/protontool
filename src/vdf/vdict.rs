
#[derive(Debug, Clone)]
pub enum VDFValue {
    String(String),
    Dict(VDFDict),
}

#[derive(Debug, Clone, Default)]
pub struct VDFDict {
    entries: Vec<(String, VDFValue)>,
}

impl VDFDict {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.entries.push((key, VDFValue::String(value)));
    }

    pub fn insert_dict(&mut self, key: String, value: VDFDict) {
        self.entries.push((key, VDFValue::Dict(value)));
    }

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

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &VDFValue)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

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
