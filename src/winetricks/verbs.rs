use std::collections::HashMap;
use std::path::Path;

use super::download::Downloader;
use super::wine::WineContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbCategory {
    App,
    Dll,
    Font,
    Setting,
    Custom,
}

impl VerbCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerbCategory::App => "apps",
            VerbCategory::Dll => "dlls",
            VerbCategory::Font => "fonts",
            VerbCategory::Setting => "settings",
            VerbCategory::Custom => "custom",
        }
    }
    
    pub fn all() -> &'static [VerbCategory] {
        &[
            VerbCategory::App,
            VerbCategory::Dll,
            VerbCategory::Font,
            VerbCategory::Setting,
            VerbCategory::Custom,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllOverride {
    Native,
    Builtin,
    NativeBuiltin,
    BuiltinNative,
}

impl DllOverride {
    pub fn as_str(&self) -> &'static str {
        match self {
            DllOverride::Native => "native",
            DllOverride::Builtin => "builtin",
            DllOverride::NativeBuiltin => "native,builtin",
            DllOverride::BuiltinNative => "builtin,native",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadFile {
    pub url: String,
    pub filename: String,
    pub sha256: Option<String>,
}

impl DownloadFile {
    pub fn new(url: &str, filename: &str, sha256: Option<&str>) -> Self {
        Self {
            url: url.to_string(),
            filename: filename.to_string(),
            sha256: sha256.map(|s| s.to_string()),
        }
    }
}

/// A local file path for offline installation (paid/licensed software)
#[derive(Debug, Clone)]
pub struct LocalFile {
    pub path: std::path::PathBuf,
    pub name: String,
}

impl LocalFile {
    pub fn new(path: &std::path::Path, name: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            name: name.to_string(),
        }
    }
}

pub type CustomAction = fn(&WineContext, &Downloader, &Path) -> Result<(), String>;
pub type BoxedAction = Box<dyn Fn(&WineContext, &Downloader, &Path) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
pub enum VerbAction {
    RunInstaller { file: DownloadFile, args: Vec<String> },
    RunLocalInstaller { file: LocalFile, args: Vec<String> },
    RunScript { script_path: std::path::PathBuf },
    Extract { file: DownloadFile, dest: String },
    ExtractCab { file: DownloadFile, dest: String, filter: Option<String> },
    Override { dll: String, mode: DllOverride },
    Registry { content: String },
    Winecfg { args: Vec<String> },
    RegisterFont { filename: String, name: String },
    CallVerb { name: String },
    Custom(CustomAction),
}

#[derive(Clone)]
pub struct Verb {
    pub name: String,
    pub category: VerbCategory,
    pub title: String,
    pub publisher: String,
    pub year: String,
    pub actions: Vec<VerbAction>,
}

impl Verb {
    pub fn new(name: &str, category: VerbCategory, title: &str, publisher: &str, year: &str) -> Self {
        Self {
            name: name.to_string(),
            category,
            title: title.to_string(),
            publisher: publisher.to_string(),
            year: year.to_string(),
            actions: Vec::new(),
        }
    }

    pub fn with_actions(mut self, actions: Vec<VerbAction>) -> Self {
        self.actions = actions;
        self
    }

    pub fn execute(&self, wine_ctx: &WineContext, cache_dir: &Path) -> Result<(), String> {
        let downloader = Downloader::new(cache_dir);
        let tmp_dir = cache_dir.join("tmp");
        std::fs::create_dir_all(&tmp_dir).ok();

        for action in &self.actions {
            execute_action(action, wine_ctx, &downloader, &tmp_dir)?;
        }
        Ok(())
    }
}

fn execute_action(action: &VerbAction, wine_ctx: &WineContext, downloader: &Downloader, tmp_dir: &Path) -> Result<(), String> {
    match action {
        VerbAction::RunInstaller { file, args } => {
            let local = downloader.download(&file.url, &file.filename, file.sha256.as_deref())?;
            let mut cmd_args: Vec<String> = vec![local.to_string_lossy().to_string()];
            cmd_args.extend(args.clone());
            let refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
            wine_ctx.run_wine(&refs).map_err(|e| e.to_string())?;
            wine_ctx.wait_for_wineserver().ok();
        }
        VerbAction::RunLocalInstaller { file, args } => {
            if !file.path.exists() {
                return Err(format!("Local installer not found: {} ({})\nPlace the installer at this path for offline installation.", file.name, file.path.display()));
            }
            let mut cmd_args: Vec<String> = vec![file.path.to_string_lossy().to_string()];
            cmd_args.extend(args.clone());
            let refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
            wine_ctx.run_wine(&refs).map_err(|e| e.to_string())?;
            wine_ctx.wait_for_wineserver().ok();
        }
        VerbAction::RunScript { script_path } => {
            if !script_path.exists() {
                return Err(format!("Script not found: {}", script_path.display()));
            }
            // Set up environment for the script
            let status = std::process::Command::new("bash")
                .arg(script_path)
                .env("WINEPREFIX", &wine_ctx.prefix_path)
                .env("WINE", &wine_ctx.wine_path)
                .env("WINESERVER", &wine_ctx.wineserver_path)
                .env("PROTON_PATH", &wine_ctx.proton_path)
                .env("W_TMP", tmp_dir)
                .env("W_CACHE", downloader.cache_dir())
                .env("W_SYSTEM32_DLLS", wine_ctx.prefix_path.join("drive_c/windows/system32"))
                .env("W_SYSTEM64_DLLS", wine_ctx.prefix_path.join("drive_c/windows/syswow64"))
                .status()
                .map_err(|e| format!("Failed to run script: {}", e))?;
            if !status.success() {
                return Err(format!("Script exited with code: {}", status.code().unwrap_or(-1)));
            }
        }
        VerbAction::Extract { file, dest } => {
            let local = downloader.download(&file.url, &file.filename, file.sha256.as_deref())?;
            let dest_path = wine_ctx.prefix_path.join(dest);
            std::fs::create_dir_all(&dest_path).ok();
            super::util::extract_archive(&local, &dest_path)?;
        }
        VerbAction::ExtractCab { file, dest, filter } => {
            let local = downloader.download(&file.url, &file.filename, file.sha256.as_deref())?;
            let dest_path = if dest.is_empty() { tmp_dir.to_path_buf() } else { wine_ctx.prefix_path.join(dest) };
            std::fs::create_dir_all(&dest_path).ok();
            super::util::extract_cab(&local, &dest_path, filter.as_deref())?;
        }
        VerbAction::Override { dll, mode } => {
            let mut ctx = wine_ctx.clone();
            ctx.set_dll_override(dll, mode.as_str());
        }
        VerbAction::Registry { content } => {
            let reg_file = tmp_dir.join("patch.reg");
            std::fs::write(&reg_file, content).map_err(|e| e.to_string())?;
            wine_ctx.run_regedit(&reg_file).map_err(|e| e.to_string())?;
            std::fs::remove_file(&reg_file).ok();
        }
        VerbAction::Winecfg { args } => {
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            wine_ctx.run_winecfg(&refs).map_err(|e| e.to_string())?;
            wine_ctx.wait_for_wineserver().ok();
        }
        VerbAction::RegisterFont { filename, name } => {
            let content = format!(
                "Windows Registry Editor Version 5.00\n\n[HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts]\n\"{} (TrueType)\"=\"{}\"\n",
                name, filename
            );
            let reg_file = tmp_dir.join("font.reg");
            std::fs::write(&reg_file, content).ok();
            wine_ctx.run_regedit(&reg_file).ok();
            std::fs::remove_file(&reg_file).ok();
        }
        VerbAction::CallVerb { .. } => { /* Handled by VerbRegistry */ }
        VerbAction::Custom(func) => { func(wine_ctx, downloader, tmp_dir)?; }
    }
    Ok(())
}

pub struct VerbRegistry {
    verbs: HashMap<String, Verb>,
}

impl VerbRegistry {
    pub fn new() -> Self {
        let mut registry = Self { verbs: HashMap::new() };
        register_settings(&mut registry);
        register_fonts(&mut registry);
        register_dlls(&mut registry);
        register_apps(&mut registry);
        
        // Load user-defined custom verbs
        for verb in super::custom::load_custom_verbs() {
            registry.register(verb);
        }
        
        registry
    }

    pub fn register(&mut self, verb: Verb) {
        self.verbs.insert(verb.name.clone(), verb);
    }

    pub fn get(&self, name: &str) -> Option<&Verb> { self.verbs.get(name) }

    pub fn list(&self, category: Option<VerbCategory>) -> Vec<&Verb> {
        match category {
            Some(cat) => self.verbs.values().filter(|v| v.category == cat).collect(),
            None => self.verbs.values().collect(),
        }
    }

    pub fn search(&self, query: &str) -> Vec<&Verb> {
        let q = query.to_lowercase();
        self.verbs.values().filter(|v| v.name.to_lowercase().contains(&q) || v.title.to_lowercase().contains(&q)).collect()
    }

    pub fn execute(&self, name: &str, wine_ctx: &WineContext, cache_dir: &Path) -> Result<(), String> {
        let verb = self.get(name).ok_or_else(|| format!("Unknown verb: {}", name))?;
        for action in &verb.actions {
            if let VerbAction::CallVerb { name: dep_name } = action {
                self.execute(dep_name, wine_ctx, cache_dir)?;
            }
        }
        verb.execute(wine_ctx, cache_dir)
    }
}

impl Default for VerbRegistry {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// SETTINGS VERBS
// ============================================================================

fn register_settings(registry: &mut VerbRegistry) {
    // Windows versions (Win7+)
    registry.register(Verb::new("win7", VerbCategory::Setting, "Set Windows version to Windows 7", "Microsoft", "2009")
        .with_actions(vec![VerbAction::Winecfg { args: vec!["-v".into(), "win7".into()] }]));
    registry.register(Verb::new("win8", VerbCategory::Setting, "Set Windows version to Windows 8", "Microsoft", "2012")
        .with_actions(vec![VerbAction::Winecfg { args: vec!["-v".into(), "win8".into()] }]));
    registry.register(Verb::new("win81", VerbCategory::Setting, "Set Windows version to Windows 8.1", "Microsoft", "2013")
        .with_actions(vec![VerbAction::Winecfg { args: vec!["-v".into(), "win81".into()] }]));
    registry.register(Verb::new("win10", VerbCategory::Setting, "Set Windows version to Windows 10", "Microsoft", "2015")
        .with_actions(vec![VerbAction::Winecfg { args: vec!["-v".into(), "win10".into()] }]));
    registry.register(Verb::new("win11", VerbCategory::Setting, "Set Windows version to Windows 11", "Microsoft", "2021")
        .with_actions(vec![VerbAction::Winecfg { args: vec!["-v".into(), "win11".into()] }]));

    // Graphics
    registry.register(Verb::new("graphics=x11", VerbCategory::Setting, "Set graphics driver to X11", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Drivers]\n\"Graphics\"=\"x11\"\n".into() }]));
    registry.register(Verb::new("graphics=wayland", VerbCategory::Setting, "Set graphics driver to Wayland", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Drivers]\n\"Graphics\"=\"wayland\"\n".into() }]));

    // Sound
    registry.register(Verb::new("sound=pulse", VerbCategory::Setting, "Set sound driver to PulseAudio", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Drivers]\n\"Audio\"=\"pulse\"\n".into() }]));
    registry.register(Verb::new("sound=alsa", VerbCategory::Setting, "Set sound driver to ALSA", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Drivers]\n\"Audio\"=\"alsa\"\n".into() }]));
    registry.register(Verb::new("sound=disabled", VerbCategory::Setting, "Disable sound", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Drivers]\n\"Audio\"=\"\"\n".into() }]));

    // Renderer
    registry.register(Verb::new("renderer=vulkan", VerbCategory::Setting, "Set renderer to Vulkan", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"renderer\"=\"vulkan\"\n".into() }]));
    registry.register(Verb::new("renderer=gl", VerbCategory::Setting, "Set renderer to OpenGL", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"renderer\"=\"gl\"\n".into() }]));
    registry.register(Verb::new("renderer=gdi", VerbCategory::Setting, "Set renderer to GDI", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"renderer\"=\"gdi\"\n".into() }]));

    // Virtual desktop
    registry.register(Verb::new("vd=off", VerbCategory::Setting, "Disable virtual desktop", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Explorer]\n\"Desktop\"=-\n[HKEY_CURRENT_USER\\Software\\Wine\\Explorer\\Desktops]\n\"Default\"=-\n".into() }]));
    for (name, size) in [("vd=640x480", "640x480"), ("vd=800x600", "800x600"), ("vd=1024x768", "1024x768"), ("vd=1280x1024", "1280x1024"), ("vd=1440x900", "1440x900")] {
        registry.register(Verb::new(name, VerbCategory::Setting, &format!("Enable virtual desktop {}", size), "Wine", "")
            .with_actions(vec![VerbAction::Registry { content: format!("Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Explorer]\n\"Desktop\"=\"Default\"\n[HKEY_CURRENT_USER\\Software\\Wine\\Explorer\\Desktops]\n\"Default\"=\"{}\"\n", size) }]));
    }

    // CSMT
    registry.register(Verb::new("csmt=on", VerbCategory::Setting, "Enable CSMT (default)", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"csmt\"=dword:00000001\n".into() }]));
    registry.register(Verb::new("csmt=off", VerbCategory::Setting, "Disable CSMT", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"csmt\"=dword:00000000\n".into() }]));

    // Font smoothing
    registry.register(Verb::new("fontsmooth=disable", VerbCategory::Setting, "Disable font smoothing", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Control Panel\\Desktop]\n\"FontSmoothing\"=\"0\"\n\"FontSmoothingType\"=dword:00000000\n".into() }]));
    registry.register(Verb::new("fontsmooth=rgb", VerbCategory::Setting, "Enable subpixel smoothing RGB", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Control Panel\\Desktop]\n\"FontSmoothing\"=\"2\"\n\"FontSmoothingType\"=dword:00000002\n\"FontSmoothingOrientation\"=dword:00000001\n".into() }]));
    registry.register(Verb::new("fontsmooth=bgr", VerbCategory::Setting, "Enable subpixel smoothing BGR", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Control Panel\\Desktop]\n\"FontSmoothing\"=\"2\"\n\"FontSmoothingType\"=dword:00000002\n\"FontSmoothingOrientation\"=dword:00000000\n".into() }]));
    registry.register(Verb::new("fontsmooth=gray", VerbCategory::Setting, "Enable grayscale smoothing", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Control Panel\\Desktop]\n\"FontSmoothing\"=\"2\"\n\"FontSmoothingType\"=dword:00000001\n".into() }]));

    // Other
    registry.register(Verb::new("nocrashdialog", VerbCategory::Setting, "Disable crash dialog", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\WineDbg]\n\"ShowCrashDialog\"=dword:00000000\n".into() }]));
    registry.register(Verb::new("mimeassoc=off", VerbCategory::Setting, "Disable MIME associations", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\FileOpenAssociations]\n\"Enable\"=\"N\"\n".into() }]));
    registry.register(Verb::new("mimeassoc=on", VerbCategory::Setting, "Enable MIME associations", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\FileOpenAssociations]\n\"Enable\"=\"Y\"\n".into() }]));
    registry.register(Verb::new("grabfullscreen=y", VerbCategory::Setting, "Force cursor clipping fullscreen", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\X11 Driver]\n\"GrabFullscreen\"=\"Y\"\n".into() }]));
    registry.register(Verb::new("grabfullscreen=n", VerbCategory::Setting, "Disable cursor clipping fullscreen", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\X11 Driver]\n\"GrabFullscreen\"=\"N\"\n".into() }]));
    registry.register(Verb::new("mwo=force", VerbCategory::Setting, "MouseWarpOverride force", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\DirectInput]\n\"MouseWarpOverride\"=\"force\"\n".into() }]));
    registry.register(Verb::new("mwo=enabled", VerbCategory::Setting, "MouseWarpOverride enabled", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\DirectInput]\n\"MouseWarpOverride\"=\"enable\"\n".into() }]));
    registry.register(Verb::new("mwo=disable", VerbCategory::Setting, "MouseWarpOverride disable", "Wine", "")
        .with_actions(vec![VerbAction::Registry { content: "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\DirectInput]\n\"MouseWarpOverride\"=\"disable\"\n".into() }]));

    // Video memory
    for size in ["512", "1024", "2048"] {
        registry.register(Verb::new(&format!("videomemorysize={}", size), VerbCategory::Setting, &format!("Set VRAM to {}MB", size), "Wine", "")
            .with_actions(vec![VerbAction::Registry { content: format!("Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Direct3D]\n\"VideoMemorySize\"=\"{}\"\n", size) }]));
    }

    // Sandbox
    registry.register(Verb::new("isolate_home", VerbCategory::Setting, "Remove links to $HOME", "Wine", "")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, _, _| {
            let users = wine_ctx.prefix_path.join("drive_c/users");
            if let Ok(entries) = std::fs::read_dir(&users) {
                for entry in entries.flatten() {
                    for subdir in ["My Documents", "Desktop", "Downloads", "My Music", "My Pictures", "My Videos"] {
                        let link = entry.path().join(subdir);
                        if link.is_symlink() {
                            std::fs::remove_file(&link).ok();
                            std::fs::create_dir_all(&link).ok();
                        }
                    }
                }
            }
            Ok(())
        })]));
}

// ============================================================================
// FONT VERBS
// ============================================================================

fn register_fonts(registry: &mut VerbRegistry) {
    registry.register(Verb::new("corefonts", VerbCategory::Font, "MS Core Fonts", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::CallVerb { name: "andale".into() },
            VerbAction::CallVerb { name: "arial".into() },
            VerbAction::CallVerb { name: "comicsans".into() },
            VerbAction::CallVerb { name: "courier".into() },
            VerbAction::CallVerb { name: "georgia".into() },
            VerbAction::CallVerb { name: "impact".into() },
            VerbAction::CallVerb { name: "times".into() },
            VerbAction::CallVerb { name: "trebuchet".into() },
            VerbAction::CallVerb { name: "verdana".into() },
            VerbAction::CallVerb { name: "webdings".into() },
        ]));

    registry.register(Verb::new("andale", VerbCategory::Font, "MS Andale Mono", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/andale32.exe", "andale32.exe", Some("0524fe42951adc3a7eb870e32f0920313c71f170c859b5f770d82b4ee111e970")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "andalemo.ttf".into(), name: "Andale Mono".into() },
        ]));
    registry.register(Verb::new("arial", VerbCategory::Font, "MS Arial", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/arial32.exe", "arial32.exe", Some("85297a4d146e9c87ac6f74822734bdee5f4b2a722d7eaa584b7f2cbf76f478f6")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "arial.ttf".into(), name: "Arial".into() },
        ]));
    registry.register(Verb::new("comicsans", VerbCategory::Font, "MS Comic Sans", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/comic32.exe", "comic32.exe", Some("9c6df3feefde26d4e41d4a4fe5db2a89f9123a772594d7f59afd062625cd204e")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "comic.ttf".into(), name: "Comic Sans MS".into() },
        ]));
    registry.register(Verb::new("courier", VerbCategory::Font, "MS Courier New", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/courie32.exe", "courie32.exe", Some("bb511d861655dde879ae552eb86b134d6fae67cb58502e6ff73ec5d9151f3384")), dest: "".into(), filter: Some("*.ttf".into()) },
            VerbAction::RegisterFont { filename: "cour.ttf".into(), name: "Courier New".into() },
        ]));
    registry.register(Verb::new("georgia", VerbCategory::Font, "MS Georgia", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/georgi32.exe", "georgi32.exe", Some("2c2c7dcda6606ea5cf08918fb7cd3f3359e9e84338dc690013f20cd42e930301")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "georgia.ttf".into(), name: "Georgia".into() },
        ]));
    registry.register(Verb::new("impact", VerbCategory::Font, "MS Impact", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/impact32.exe", "impact32.exe", Some("6061ef3b7401d9642f5dfdb5f2b376aa14663f6275e60a51207ad4facf2fccfb")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "impact.ttf".into(), name: "Impact".into() },
        ]));
    registry.register(Verb::new("times", VerbCategory::Font, "MS Times New Roman", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/times32.exe", "times32.exe", Some("db56595ec6ef5d3de5c24994f001f03b2a13e37cee27bc25c58f6f43e8f807ab")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "times.ttf".into(), name: "Times New Roman".into() },
        ]));
    registry.register(Verb::new("trebuchet", VerbCategory::Font, "MS Trebuchet", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/trebuc32.exe", "trebuc32.exe", Some("5a690d9bb8510be1b8b4c025b7f34b90e9e2c881c05c8b8a5a3052525b8a4c5a")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "trebuc.ttf".into(), name: "Trebuchet MS".into() },
        ]));
    registry.register(Verb::new("verdana", VerbCategory::Font, "MS Verdana", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/verdan32.exe", "verdan32.exe", Some("c1cb61255e363166794e47664e2f21af8e3a26cb6346eb8d2ae2fa85dd5aad96")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "verdana.ttf".into(), name: "Verdana".into() },
        ]));
    registry.register(Verb::new("webdings", VerbCategory::Font, "MS Webdings", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://github.com/pushcx/corefonts/raw/master/webdin32.exe", "webdin32.exe", Some("64595b5abc1080fba8610c5c34fab5863408e806aafe84653ca8575f82ca9ab6")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "webdings.ttf".into(), name: "Webdings".into() },
        ]));
    registry.register(Verb::new("tahoma", VerbCategory::Font, "MS Tahoma", "Microsoft", "1999")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://downloads.sourceforge.net/corefonts/OldFiles/IELPKTH.CAB", "IELPKTH.CAB", Some("c1be3fb8f0042570be76ec6daa03a99142c88367c1bc810240b85827c715961a")), dest: "".into(), filter: Some("*.TTF".into()) },
            VerbAction::RegisterFont { filename: "tahoma.ttf".into(), name: "Tahoma".into() },
        ]));
    registry.register(Verb::new("lucida", VerbCategory::Font, "MS Lucida Console", "Microsoft", "1998")
        .with_actions(vec![
            VerbAction::ExtractCab { file: DownloadFile::new("https://downloads.sourceforge.net/corefonts/OldFiles/IELPKTH.CAB", "IELPKTH.CAB", Some("c1be3fb8f0042570be76ec6daa03a99142c88367c1bc810240b85827c715961a")), dest: "".into(), filter: Some("lucon.ttf".into()) },
            VerbAction::RegisterFont { filename: "lucon.ttf".into(), name: "Lucida Console".into() },
        ]));
}

// ============================================================================
// DLL VERBS
// ============================================================================

fn register_dlls(registry: &mut VerbRegistry) {
    // Visual C++ Runtimes
    registry.register(Verb::new("vcrun2022", VerbCategory::Dll, "Visual C++ 2015-2022 Runtime", "Microsoft", "2022")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://aka.ms/vs/17/release/vc_redist.x86.exe", "vc_redist.x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://aka.ms/vs/17/release/vc_redist.x64.exe", "vc_redist.x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("vcrun2019", VerbCategory::Dll, "Visual C++ 2015-2019 Runtime", "Microsoft", "2019")
        .with_actions(vec![VerbAction::CallVerb { name: "vcrun2022".into() }]));
    registry.register(Verb::new("vcrun2017", VerbCategory::Dll, "Visual C++ 2017 Runtime", "Microsoft", "2017")
        .with_actions(vec![VerbAction::CallVerb { name: "vcrun2022".into() }]));
    registry.register(Verb::new("vcrun2015", VerbCategory::Dll, "Visual C++ 2015 Runtime", "Microsoft", "2015")
        .with_actions(vec![VerbAction::CallVerb { name: "vcrun2022".into() }]));

    // .NET Framework
    registry.register(Verb::new("dotnet48", VerbCategory::Dll, "MS .NET 4.8", "Microsoft", "2019")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/2d6bb6b2-226a-4baa-bdec-798822606ff1/8494001c276a4b96804cde7829c04d7f/ndp48-x86-x64-allos-enu.exe", "ndp48-x86-x64-allos-enu.exe", Some("68c9986a8dcc0214d909aa1f31bee9fb5461bb839edca996a75b08ddffc1483f")),
            args: vec!["/q".into(), "/norestart".into()],
        }]));
    registry.register(Verb::new("dotnet472", VerbCategory::Dll, "MS .NET 4.7.2", "Microsoft", "2018")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/6/E/4/6E48E8AB-DC00-419E-9704-06DD46E5F81D/NDP472-KB4054530-x86-x64-AllOS-ENU.exe", "NDP472-KB4054530-x86-x64-AllOS-ENU.exe", Some("c908f0a5bea4be282e35acba307d0061b71b8b66ca9894943d3cbb53cad019bc")),
            args: vec!["/q".into(), "/norestart".into()],
        }]));
    registry.register(Verb::new("dotnet40", VerbCategory::Dll, "MS .NET 4.0", "Microsoft", "2011")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/9/5/A/95A9616B-7A37-4AF6-BC36-D6EA96C8DAAE/dotNetFx40_Full_x86_x64.exe", "dotNetFx40_Full_x86_x64.exe", Some("65e064258f2e418816b304f646ff9e87af101e4c9552ab064bb74d281c38659f")),
            args: vec!["/q".into(), "/norestart".into()],
        }]));

    // DXVK
    registry.register(Verb::new("dxvk", VerbCategory::Dll, "DXVK (latest)", "Philip Rebohle", "2024")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://github.com/doitsujin/dxvk/releases/download/v2.5.3/dxvk-2.5.3.tar.gz", "dxvk-2.5.3.tar.gz", None)?;
            crate::winetricks::util::extract_archive(&file, tmp_dir)?;
            let dxvk = tmp_dir.join("dxvk-2.5.3");
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for dll in ["d3d9.dll", "d3d10core.dll", "d3d11.dll", "dxgi.dll"] {
                if syswow.exists() {
                    std::fs::copy(dxvk.join("x32").join(dll), syswow.join(dll)).ok();
                    std::fs::copy(dxvk.join("x64").join(dll), sys32.join(dll)).ok();
                } else {
                    std::fs::copy(dxvk.join("x32").join(dll), sys32.join(dll)).ok();
                }
            }
            let mut ctx = wine_ctx.clone();
            for dll in ["d3d9", "d3d10core", "d3d11", "dxgi"] { ctx.set_dll_override(dll, "native"); }
            Ok(())
        })]));

    // PhysX
    registry.register(Verb::new("physx", VerbCategory::Dll, "PhysX", "Nvidia", "2021")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://us.download.nvidia.com/Windows/9.21.0713/PhysX-9.21.0713-SystemSoftware.exe", "PhysX-9.21.0713-SystemSoftware.exe", None),
            args: vec!["/s".into()],
        }]));

    // XNA
    registry.register(Verb::new("xna40", VerbCategory::Dll, "XNA Framework 4.0", "Microsoft", "2010")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/A/C/2/AC2C903B-E6E8-42C2-9FD7-BEBAC362A930/xnafx40_redist.msi", "xnafx40_redist.msi", Some("89eb4cae2a051f127e41f223c9bab6ce7fbd8ff2d9bb8e7e5f90f1e0b8d85b2f")),
            args: vec!["/quiet".into()],
        }]));

    // OpenAL
    registry.register(Verb::new("openal", VerbCategory::Dll, "OpenAL Runtime", "Creative", "2023")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://www.openal.org/downloads/oalinst.zip", "oalinst.zip", None),
            args: vec!["/s".into()],
        }]));

    // Older Visual C++ Runtimes
    registry.register(Verb::new("vcrun2013", VerbCategory::Dll, "Visual C++ 2013 Runtime", "Microsoft", "2013")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/2/E/6/2E61CFA4-993B-4DD4-91DA-3737CD5CD6E3/vcredist_x86.exe", "vcredist_2013_x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/2/E/6/2E61CFA4-993B-4DD4-91DA-3737CD5CD6E3/vcredist_x64.exe", "vcredist_2013_x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("vcrun2012", VerbCategory::Dll, "Visual C++ 2012 Runtime", "Microsoft", "2012")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x86.exe", "vcredist_2012_x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x64.exe", "vcredist_2012_x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("vcrun2010", VerbCategory::Dll, "Visual C++ 2010 Runtime", "Microsoft", "2010")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x86.exe", "vcredist_2010_x86.exe", None), args: vec!["/q".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x64.exe", "vcredist_2010_x64.exe", None), args: vec!["/q".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("vcrun2008", VerbCategory::Dll, "Visual C++ 2008 Runtime", "Microsoft", "2008")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/5/D/8/5D8C65CB-C849-4025-8E95-C3966CAFD8AE/vcredist_x86.exe", "vcredist_2008_x86.exe", None), args: vec!["/q".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/5/D/8/5D8C65CB-C849-4025-8E95-C3966CAFD8AE/vcredist_x64.exe", "vcredist_2008_x64.exe", None), args: vec!["/q".into()] },
        ]));
    registry.register(Verb::new("vcrun2005", VerbCategory::Dll, "Visual C++ 2005 Runtime", "Microsoft", "2005")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/8/B/4/8B42259F-5D70-43F4-AC2E-4B208FD8D66A/vcredist_x86.EXE", "vcredist_2005_x86.exe", None), args: vec!["/q".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.microsoft.com/download/8/B/4/8B42259F-5D70-43F4-AC2E-4B208FD8D66A/vcredist_x64.EXE", "vcredist_2005_x64.exe", None), args: vec!["/q".into()] },
        ]));

    // More .NET versions
    registry.register(Verb::new("dotnet46", VerbCategory::Dll, "MS .NET 4.6", "Microsoft", "2015")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/6/F/9/6F9673B1-87D1-46C4-BF04-95F24C3EB9DA/enu_netfx/NDP46-KB3045557-x86-x64-AllOS-ENU_exe/NDP46-KB3045557-x86-x64-AllOS-ENU.exe", "NDP46-KB3045557-x86-x64-AllOS-ENU.exe", None),
            args: vec!["/q".into(), "/norestart".into()],
        }]));
    registry.register(Verb::new("dotnet462", VerbCategory::Dll, "MS .NET 4.6.2", "Microsoft", "2016")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/8e396c75-4d0d-41d3-aea8-848babc2736a/80b431456d8866ebe053eb8b81a168b3/ndp462-kb3151800-x86-x64-allos-enu.exe", "NDP462-KB3151800-x86-x64-AllOS-ENU.exe", None),
            args: vec!["/q".into(), "/norestart".into()],
        }]));
    registry.register(Verb::new("dotnet35sp1", VerbCategory::Dll, "MS .NET 3.5 SP1", "Microsoft", "2008")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/0/6/1/061F001C-8752-4600-A198-53214C69B51F/dotnetfx35setup.exe", "dotnetfx35setup.exe", None),
            args: vec!["/q".into()],
        }]));

    // .NET Core / .NET 6+
    registry.register(Verb::new("dotnet6", VerbCategory::Dll, "MS .NET Runtime 6.0", "Microsoft", "2023")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/c8af603e-ef3d-4bf4-9c09-26a5de6f3c87/680348e491ff4206daf8064406d6841a/dotnet-runtime-6.0.36-win-x86.exe", "dotnet-runtime-6.0.36-win-x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/61747fc6-7236-4d5d-a1c8-81f953b3d22a/6dc2e68a7519e9effb54c8c0e3e96e5f/dotnet-runtime-6.0.36-win-x64.exe", "dotnet-runtime-6.0.36-win-x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("dotnet7", VerbCategory::Dll, "MS .NET Runtime 7.0", "Microsoft", "2023")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/4986134e-391c-4121-aabc-c60ef5d048af/5354323f0a90fc4bf98fed19429aa803/dotnet-runtime-7.0.20-win-x86.exe", "dotnet-runtime-7.0.20-win-x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/abe74d39-d26f-4a5f-a0e8-80e00a8a7885/d5dc5f5f1e5c3adfbb43dbbe41168a5a/dotnet-runtime-7.0.20-win-x64.exe", "dotnet-runtime-7.0.20-win-x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("dotnet8", VerbCategory::Dll, "MS .NET Runtime 8.0", "Microsoft", "2024")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/6e1f5faf-ee7e-4869-b480-41eb458cf09f/ae8ee33cc3b0b1b11a8180f0e08e7390/dotnet-runtime-8.0.11-win-x86.exe", "dotnet-runtime-8.0.11-win-x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/53d7acb6-48a5-4328-8d0b-e5045b96b9bc/a10d41d8ad07d317b8eed6cf4e63d5c2/dotnet-runtime-8.0.11-win-x64.exe", "dotnet-runtime-8.0.11-win-x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));
    registry.register(Verb::new("dotnetdesktop8", VerbCategory::Dll, "MS .NET Desktop Runtime 8.0", "Microsoft", "2024")
        .with_actions(vec![
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/04af55e3-4874-4e62-9bfc-c0a77bfd47f9/1b28c7c9928dec736a10fbd343b67b1e/windowsdesktop-runtime-8.0.11-win-x86.exe", "windowsdesktop-runtime-8.0.11-win-x86.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
            VerbAction::RunInstaller { file: DownloadFile::new("https://download.visualstudio.microsoft.com/download/pr/27bcdd70-ce64-4049-ba24-2b14f9267729/d4a435e55182ce5424757bffc0bfc6b0/windowsdesktop-runtime-8.0.11-win-x64.exe", "windowsdesktop-runtime-8.0.11-win-x64.exe", None), args: vec!["/install".into(), "/quiet".into(), "/norestart".into()] },
        ]));

    // vkd3d (Vulkan D3D12)
    registry.register(Verb::new("vkd3d", VerbCategory::Dll, "vkd3d (Vulkan D3D12)", "Hans-Kristian Arntzen", "2024")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://github.com/HansKristian-Work/vkd3d-proton/releases/download/v2.13/vkd3d-proton-2.13.tar.zst", "vkd3d-proton-2.13.tar.zst", None)?;
            crate::winetricks::util::extract_archive(&file, tmp_dir)?;
            let vkd3d = tmp_dir.join("vkd3d-proton-2.13");
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for dll in ["d3d12.dll", "d3d12core.dll"] {
                if syswow.exists() {
                    std::fs::copy(vkd3d.join("x86").join(dll), syswow.join(dll)).ok();
                    std::fs::copy(vkd3d.join("x64").join(dll), sys32.join(dll)).ok();
                } else {
                    std::fs::copy(vkd3d.join("x86").join(dll), sys32.join(dll)).ok();
                }
            }
            Ok(())
        })]));

    // FAudio
    registry.register(Verb::new("faudio", VerbCategory::Dll, "FAudio (XAudio reimplementation)", "Kron4ek", "2020")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://github.com/Kron4ek/FAudio-Builds/releases/download/20.07/faudio-20.07.tar.xz", "faudio-20.07.tar.xz", None)?;
            crate::winetricks::util::extract_archive(&file, tmp_dir)?;
            let faudio = tmp_dir.join("faudio-20.07");
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for dll in ["FAudio.dll", "XAudio2_0.dll", "XAudio2_1.dll", "XAudio2_2.dll", "XAudio2_3.dll", "XAudio2_4.dll", "XAudio2_5.dll", "XAudio2_6.dll", "XAudio2_7.dll", "XAudio2_8.dll", "XAudio2_9.dll", "xaudio2_9redist.dll"] {
                if let Ok(_) = std::fs::copy(faudio.join("x32").join(dll), if syswow.exists() { syswow.join(dll) } else { sys32.join(dll) }) {}
                if syswow.exists() {
                    std::fs::copy(faudio.join("x64").join(dll), sys32.join(dll)).ok();
                }
            }
            Ok(())
        })]));

    // DirectX June 2010 redistributable verbs
    registry.register(Verb::new("d3dx9", VerbCategory::Dll, "MS d3dx9 from DirectX 9 redistributable", "Microsoft", "2010")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe", "directx_Jun2010_redist.exe", Some("8746ee1a84a083a90e37899d71d50d5c7c015e69688a466aa80447f011780c0d"))?;
            // Extract the main archive
            let status = std::process::Command::new("7z").args(["x", "-y", &file.to_string_lossy(), "-o", &tmp_dir.to_string_lossy()]).status();
            if status.is_err() || !status.unwrap().success() {
                // Try cabextract as fallback
                std::process::Command::new("cabextract").args(["-d", &tmp_dir.to_string_lossy(), "-F", "*d3dx9*", &file.to_string_lossy()]).status().ok();
            }
            // Extract d3dx9 DLLs from cab files
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for entry in std::fs::read_dir(tmp_dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "cab") {
                    let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
                    if name.contains("d3dx9") {
                        if name.contains("x64") && syswow.exists() {
                            std::process::Command::new("cabextract").args(["-d", &sys32.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                        } else {
                            let dest = if syswow.exists() { &syswow } else { &sys32 };
                            std::process::Command::new("cabextract").args(["-d", &dest.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                        }
                    }
                }
            }
            Ok(())
        })]));

    // xinput
    registry.register(Verb::new("xinput", VerbCategory::Dll, "Microsoft XInput (Xbox controller support)", "Microsoft", "2010")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe", "directx_Jun2010_redist.exe", Some("8746ee1a84a083a90e37899d71d50d5c7c015e69688a466aa80447f011780c0d"))?;
            std::process::Command::new("cabextract").args(["-d", &tmp_dir.to_string_lossy(), "-F", "*xinput*", &file.to_string_lossy()]).status().ok();
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for entry in std::fs::read_dir(tmp_dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "cab") {
                    let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
                    if name.contains("xinput") {
                        if name.contains("x64") && syswow.exists() {
                            std::process::Command::new("cabextract").args(["-d", &sys32.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                        } else {
                            let dest = if syswow.exists() { &syswow } else { &sys32 };
                            std::process::Command::new("cabextract").args(["-d", &dest.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                        }
                    }
                }
            }
            Ok(())
        })]));

    // d3dcompiler_47
    registry.register(Verb::new("d3dcompiler_47", VerbCategory::Dll, "MS d3dcompiler_47.dll", "Microsoft", "2019")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            // Download from a known source
            let file = downloader.download("https://github.com/AlicanAky662/d3dcompiler_47/releases/download/2024.12.08/d3dcompiler_47.zip", "d3dcompiler_47.zip", None)?;
            crate::winetricks::util::extract_archive(&file, tmp_dir)?;
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            if syswow.exists() {
                std::fs::copy(tmp_dir.join("x86/d3dcompiler_47.dll"), syswow.join("d3dcompiler_47.dll")).ok();
                std::fs::copy(tmp_dir.join("x64/d3dcompiler_47.dll"), sys32.join("d3dcompiler_47.dll")).ok();
            } else {
                std::fs::copy(tmp_dir.join("x86/d3dcompiler_47.dll"), sys32.join("d3dcompiler_47.dll")).ok();
            }
            Ok(())
        })]));

    // d3dcompiler_43
    registry.register(Verb::new("d3dcompiler_43", VerbCategory::Dll, "MS d3dcompiler_43.dll", "Microsoft", "2010")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            let file = downloader.download("https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe", "directx_Jun2010_redist.exe", Some("8746ee1a84a083a90e37899d71d50d5c7c015e69688a466aa80447f011780c0d"))?;
            std::process::Command::new("cabextract").args(["-d", &tmp_dir.to_string_lossy(), "-F", "*d3dcompiler_43*", &file.to_string_lossy()]).status().ok();
            let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
            let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
            for entry in std::fs::read_dir(tmp_dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "cab") {
                    let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
                    if name.contains("x64") && syswow.exists() {
                        std::process::Command::new("cabextract").args(["-d", &sys32.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                    } else {
                        let dest = if syswow.exists() { &syswow } else { &sys32 };
                        std::process::Command::new("cabextract").args(["-d", &dest.to_string_lossy(), "-F", "*.dll", &path.to_string_lossy()]).status().ok();
                    }
                }
            }
            Ok(())
        })]));

    // GDI+
    registry.register(Verb::new("gdiplus", VerbCategory::Dll, "MS GDI+", "Microsoft", "2011")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/a/a/c/aac39226-8825-44ce-90e3-bf8203e74006/WindowsXP-KB975337-x86-ENU.exe", "WindowsXP-KB975337-x86-ENU.exe", None),
            args: vec!["/extract".into(), "/quiet".into()],
        }]));

    // Media Foundation
    registry.register(Verb::new("mf", VerbCategory::Dll, "MS Media Foundation", "Microsoft", "2011")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, _, _| {
            // Enable Media Foundation DLLs via registry
            let reg_content = r#"Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\Software\Microsoft\Windows Media Foundation]

[HKEY_LOCAL_MACHINE\Software\Microsoft\Windows Media Foundation\HardwareMFT]
"#;
            let reg_file = wine_ctx.prefix_path.join("drive_c/mf.reg");
            std::fs::write(&reg_file, reg_content).ok();
            wine_ctx.run_regedit(&reg_file).ok();
            std::fs::remove_file(&reg_file).ok();
            Ok(())
        })]));

    // quartz (DirectShow)
    registry.register(Verb::new("quartz", VerbCategory::Dll, "MS quartz.dll (DirectShow)", "Microsoft", "2011")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, _, _| {
            // Just set native override - Wine has a builtin
            let mut ctx = wine_ctx.clone();
            ctx.set_dll_override("quartz", "native,builtin");
            Ok(())
        })]));

    // Visual Basic 6 Runtime
    registry.register(Verb::new("vb6run", VerbCategory::Dll, "MS Visual Basic 6 Runtime", "Microsoft", "2004")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/5/a/d/5ad868a0-8ecd-4bb0-a882-fe53eb7ef348/VB6.0-KB290887-X86.exe", "VB6.0-KB290887-X86.exe", None),
            args: vec!["/q".into()],
        }]));

    // XNA 3.1
    registry.register(Verb::new("xna31", VerbCategory::Dll, "XNA Framework 3.1", "Microsoft", "2009")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://download.microsoft.com/download/D/C/2/DC2F9B1E-1A2D-4CF4-8E28-F3B8B5D71930/xnafx31_redist.msi", "xnafx31_redist.msi", None),
            args: vec!["/quiet".into()],
        }]));

    // DXVK versioned - helper function
    fn install_dxvk(wine_ctx: &crate::winetricks::WineContext, downloader: &crate::winetricks::download::Downloader, tmp_dir: &std::path::Path, version: &str, url: &str) -> Result<(), String> {
        let filename = format!("dxvk-{}.tar.gz", version);
        let file = downloader.download(url, &filename, None)?;
        crate::winetricks::util::extract_archive(&file, tmp_dir)?;
        let dxvk = tmp_dir.join(format!("dxvk-{}", version));
        let sys32 = wine_ctx.prefix_path.join("drive_c/windows/system32");
        let syswow = wine_ctx.prefix_path.join("drive_c/windows/syswow64");
        for dll in ["d3d9.dll", "d3d10core.dll", "d3d11.dll", "dxgi.dll"] {
            if syswow.exists() {
                std::fs::copy(dxvk.join("x32").join(dll), syswow.join(dll)).ok();
                std::fs::copy(dxvk.join("x64").join(dll), sys32.join(dll)).ok();
            } else {
                std::fs::copy(dxvk.join("x32").join(dll), sys32.join(dll)).ok();
            }
        }
        Ok(())
    }

    registry.register(Verb::new("dxvk2060", VerbCategory::Dll, "DXVK 2.6", "Philip Rebohle", "2024")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            install_dxvk(wine_ctx, downloader, tmp_dir, "2.6", "https://github.com/doitsujin/dxvk/releases/download/v2.6/dxvk-2.6.tar.gz")
        })]));
    registry.register(Verb::new("dxvk2050", VerbCategory::Dll, "DXVK 2.5", "Philip Rebohle", "2024")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            install_dxvk(wine_ctx, downloader, tmp_dir, "2.5", "https://github.com/doitsujin/dxvk/releases/download/v2.5/dxvk-2.5.tar.gz")
        })]));
    registry.register(Verb::new("dxvk2040", VerbCategory::Dll, "DXVK 2.4", "Philip Rebohle", "2024")
        .with_actions(vec![VerbAction::Custom(|wine_ctx, downloader, tmp_dir| {
            install_dxvk(wine_ctx, downloader, tmp_dir, "2.4", "https://github.com/doitsujin/dxvk/releases/download/v2.4/dxvk-2.4.tar.gz")
        })]));
}

// ============================================================================
// APP VERBS
// ============================================================================

fn register_apps(registry: &mut VerbRegistry) {
    registry.register(Verb::new("7zip", VerbCategory::App, "7-Zip", "Igor Pavlov", "2024")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://www.7-zip.org/a/7z2409-x64.exe", "7z2409-x64.exe", None),
            args: vec!["/S".into()],
        }]));
    registry.register(Verb::new("vlc", VerbCategory::App, "VLC media player", "VideoLAN", "2015")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://get.videolan.org/vlc/3.0.21/win64/vlc-3.0.21-win64.exe", "vlc-3.0.21-win64.exe", None),
            args: vec!["/S".into()],
        }]));
    registry.register(Verb::new("winrar", VerbCategory::App, "WinRAR", "RARLAB", "1993")
        .with_actions(vec![VerbAction::RunInstaller {
            file: DownloadFile::new("https://www.rarlab.com/rar/winrar-x64-701.exe", "winrar-x64-701.exe", None),
            args: vec!["/s".into()],
        }]));
}
