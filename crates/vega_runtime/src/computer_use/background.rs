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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({
            "exact_window":{"status":"matched","pid":1,"window_id":2},
            "routes":[
                {"route":"accessibility","status":"available"},
                {"route":"window_pointer","status":"available"},
                {"route":"pid_keyboard","status":"refused","reason":"same_pid_keyboard_ambiguity"}
            ],
            "observation":{"one_shot_capture":"available","frame_freshness":"unknown"},
            "escalation":"SECRET_RAW_REASON"
        })
    }

    #[test]
    fn issue75_background_report_is_target_bound_finite_and_content_free() {
        let valid = BackgroundInput::from_report(&report(), 1, 2);
        assert_eq!(
            valid.keyboard_refusal(),
            Some(Class::SamePidKeyboardAmbiguity)
        );
        assert!(valid.model_text().contains("accessibility=available"));
        assert!(valid.model_text().contains("window_pointer=available"));
        assert!(!valid.model_text().contains("SECRET"));
        assert!(!format!("{valid:?}").contains("SECRET"));
        let mut cases = vec![Value::Null];
        for (field, value) in [
            ("pid", json!(9)),
            ("window_id", json!(9)),
            ("status", json!("unknown")),
        ] {
            let mut malformed = report();
            malformed["exact_window"][field] = value;
            cases.push(malformed);
        }
        for (field, value) in [
            ("status", json!("unknown")),
            ("reason", json!("SECRET_UNKNOWN")),
            ("reason", json!({"text":"SECRET_RAW_REASON"})),
        ] {
            let mut malformed = report();
            malformed["routes"][2][field] = value;
            cases.push(malformed);
        }
        let mut duplicate = report();
        duplicate["routes"][1] = duplicate["routes"][2].clone();
        cases.push(duplicate);
        let mut missing = report();
        missing["routes"].as_array_mut().unwrap().pop();
        cases.push(missing);
        for malformed in cases {
            let projected = BackgroundInput::from_report(&malformed, 1, 2);
            assert_eq!(projected.keyboard_refusal(), None);
            assert!(projected.model_text().contains("pid_keyboard=unknown"));
            assert!(!projected.model_text().contains("SECRET"));
            assert!(!format!("{projected:?}").contains("SECRET"));
        }
    }
}
