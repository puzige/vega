use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

static CHINESE: AtomicBool = AtomicBool::new(false);
static CATALOGS: LazyLock<(Catalog, Catalog)> = LazyLock::new(|| {
    (
        load_catalog([
            include_str!("../locales/en.toml"),
            include_str!("../locales/ui.en.toml"),
            include_str!("../locales/app.en.toml"),
            include_str!("../locales/domain.en.toml"),
        ]),
        load_catalog([
            include_str!("../locales/zh-CN.toml"),
            include_str!("../locales/ui.zh-CN.toml"),
            include_str!("../locales/app.zh-CN.toml"),
            include_str!("../locales/domain.zh-CN.toml"),
        ]),
    )
});

type Catalog = BTreeMap<String, String>;

fn load_catalog(sources: [&str; 4]) -> Catalog {
    let mut catalog = Catalog::new();
    for source in sources {
        match toml::from_str::<Catalog>(source) {
            Ok(entries) => {
                for (key, value) in entries {
                    let previous = catalog.insert(key, value);
                    debug_assert!(previous.is_none(), "duplicate translation key");
                }
            }
            Err(error) => debug_assert!(false, "invalid translation catalog: {error}"),
        }
    }
    catalog
}

pub fn set_language(language: &str) {
    CHINESE.store(language == "zh-CN", Ordering::Relaxed);
}

pub fn language() -> &'static str {
    if CHINESE.load(Ordering::Relaxed) {
        "zh-CN"
    } else {
        "en"
    }
}

pub fn t(key: &'static str) -> &'static str {
    let (english, chinese) = &*CATALOGS;
    let Some(fallback) = english.get(key) else {
        debug_assert!(false, "missing English translation: {key}");
        return key;
    };
    if CHINESE.load(Ordering::Relaxed) {
        match chinese.get(key) {
            Some(value) => value,
            None => {
                debug_assert!(false, "missing Chinese translation: {key}");
                fallback
            }
        }
    } else {
        fallback
    }
}

pub fn t_args(key: &'static str, args: &[(&str, &str)]) -> String {
    let template = t(key);
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                result.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                result.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for character in chars.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }
                    name.push(character);
                }
                if closed {
                    if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
                        result.push_str(value);
                    } else {
                        debug_assert!(false, "missing translation argument: {key}.{name}");
                        result.push('{');
                        result.push_str(&name);
                        result.push('}');
                    }
                } else {
                    debug_assert!(false, "unclosed translation argument: {key}");
                    result.push('{');
                    result.push_str(&name);
                }
            }
            character => result.push(character),
        }
    }
    result
}
