use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn load_text(text: &str) -> LoadedConfig {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = loop {
        let path = std::env::temp_dir().join(format!(
            "hctui-config-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => break Scratch(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create owned config scratch directory: {error}"),
        }
    };
    let path = root.0.join("config.toml");
    std::fs::write(&path, text).unwrap();
    let loaded = load(&path);
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    loaded
}

#[test]
fn known_deferred_sections_do_not_hide_unknown_root_keys() {
    let loaded = load_text(
        "bogus = true\nplugin = []\n[terminal]\ndefault_shell = 'sh'\n[ui]\nsidebar_width = 30\n",
    );
    assert_eq!(loaded.config.ui.sidebar_width, 30);
    assert_eq!(
        loaded.diagnostics,
        [
            "unknown config key bogus; ignoring key",
            "unknown config key plugin; ignoring key",
        ]
    );
}

#[test]
fn unknown_section_uses_the_reference_hint_once() {
    let loaded = load_text("[toast]\ndelivery = 'system'\n");
    assert_eq!(
        loaded.diagnostics,
        ["unknown config section [toast]; did you mean [ui.toast]? ignoring section"]
    );
}

#[test]
fn displaced_bom_is_recovered_and_string_bom_is_preserved() {
    let loaded = load_text("onboarding = false\n\u{feff}[ui]\nsidebar_width = 30\n");
    assert_eq!(loaded.config.ui.sidebar_width, 30);
    assert!(loaded.diagnostics.is_empty());
    let text = "[theme]\nname = '''tokyo-\u{feff}night'''\n";
    let loaded = load_text(text);
    assert_eq!(
        loaded.config.theme.name.as_deref(),
        Some("tokyo-\u{feff}night")
    );
}
