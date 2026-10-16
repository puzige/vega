use super::diagnostics::{Class, background_refusal};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Route {
    #[default]
    Unknown,
    Available,
    Refused(Class),
}

impl Route {
    fn project(self) -> String {
        match self {
            Self::Unknown => "unknown".into(),
            Self::Available => "available".into(),
            Self::Refused(class) => format!("refused:{}", class.code()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BackgroundInput {
    accessibility: Route,
    pointer: Route,
    keyboard: Route,
}

impl BackgroundInput {
    pub(super) fn from_report(report: &Value, pid: u32, window_id: u32) -> Self {
        if report["exact_window"]["status"] != "matched"
            || report["exact_window"]["pid"].as_u64() != Some(u64::from(pid))
            || report["exact_window"]["window_id"].as_u64() != Some(u64::from(window_id))
        {
            return Self::default();
        }
        let Some(routes) = report["routes"].as_array() else {
            return Self::default();
        };
        if routes.len() != 3 {
            return Self::default();
        }
        let parse = |name: &str| {
            let mut entries = routes.iter().filter(|route| route["route"] == name);
            let Some(route) = entries.next() else {
                return Route::Unknown;
            };
            if entries.next().is_some() {
                return Route::Unknown;
            }
            match route["status"].as_str() {
                Some("available") if route.get("reason").is_none() => Route::Available,
                Some("refused") => route["reason"]
                    .as_str()
                    .and_then(background_refusal)
                    .map_or(Route::Unknown, Route::Refused),
                _ => Route::Unknown,
            }
        };
        Self {
            accessibility: parse("accessibility"),
            pointer: parse("window_pointer"),
            keyboard: parse("pid_keyboard"),
        }
    }

    pub(super) fn keyboard_refusal(self) -> Option<Class> {
        match self.keyboard {
            Route::Refused(class) => Some(class),
            Route::Unknown | Route::Available => None,
        }
    }

    pub(super) fn model_text(self) -> String {
        format!(
            "Background input routes at observation time: accessibility={}; window_pointer={}; pid_keyboard={}. This report is advisory; the executor rechecks before every action. When pid_keyboard is refused, do not use hotkey or press_key; scroll requires an element target. type_text and element-targeted scroll may use accessibility semantics; use an observed element when appropriate. Unknown is not proof of availability.",
            self.accessibility.project(),
            self.pointer.project(),
            self.keyboard.project()
        )
    }
}
