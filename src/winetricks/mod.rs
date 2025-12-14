pub mod verbs;
pub mod download;
pub mod wine;
pub mod registry;
pub mod util;
pub mod custom;

use std::path::{Path, PathBuf};

use crate::steam::ProtonApp;

pub use verbs::{Verb, VerbCategory, VerbRegistry};
pub use wine::{WineContext, WineArch};

pub fn get_winetricks_path() -> Option<PathBuf> {
    get_external_winetricks_path()
}

pub fn get_external_winetricks_path() -> Option<PathBuf> {
    if let Ok(winetricks) = std::env::var("WINETRICKS") {
        let path = PathBuf::from(winetricks);
        if path.exists() {
            return Some(path);
        }
    }

    crate::util::which("winetricks")
}

pub struct Winetricks {
    pub wine_ctx: WineContext,
    pub cache_dir: PathBuf,
    pub verb_registry: VerbRegistry,
}

impl Winetricks {
    pub fn new(proton_app: &ProtonApp, prefix_path: &Path) -> Self {
        Self::new_with_arch(proton_app, prefix_path, wine::WineArch::Win64)
    }
    
    pub fn new_with_arch(proton_app: &ProtonApp, prefix_path: &Path, arch: wine::WineArch) -> Self {
        let wine_ctx = WineContext::from_proton_with_arch(proton_app, prefix_path, arch);
        
        let cache_dir = crate::config::get_cache_dir().join("winetricks");
        std::fs::create_dir_all(&cache_dir).ok();
        
        let verb_registry = VerbRegistry::new();
        
        Self {
            wine_ctx,
            cache_dir,
            verb_registry,
        }
    }

    pub fn run_verb(&self, verb_name: &str) -> Result<(), String> {
        let verb = self.verb_registry.get(verb_name)
            .ok_or_else(|| format!("Unknown verb: {}", verb_name))?;
        
        verb.execute(&self.wine_ctx, &self.cache_dir)
    }

    pub fn list_verbs(&self, category: Option<VerbCategory>) -> Vec<&Verb> {
        self.verb_registry.list(category)
    }

    pub fn search_verbs(&self, query: &str) -> Vec<&Verb> {
        self.verb_registry.search(query)
    }
}
