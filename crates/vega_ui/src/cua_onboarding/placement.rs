#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }

    pub fn intersects(&self, other: Rect) -> bool {
        self.x < other.max_x()
            && other.x < self.max_x()
            && self.y < other.max_y()
            && other.y < self.max_y()
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.x..self.max_x()).contains(&x) && (self.y..self.max_y()).contains(&y)
    }
}

pub const CARD_SHADOW_PADDING: f64 = 24.0;
pub const CARD_INSET: f64 = 18.0;
pub const FOLLOW_EPSILON: f64 = 1.0;

pub fn perch(settings: Option<Rect>, card: (f64, f64), visible: Rect) -> (f64, f64) {
    let origin = match settings {
        Some(settings) => (
            settings.max_x() - CARD_INSET + CARD_SHADOW_PADDING - card.0,
            settings.max_y() - CARD_INSET + CARD_SHADOW_PADDING - card.1,
        ),
        None => (
            visible.max_x() + CARD_SHADOW_PADDING - card.0 - 24.0,
            visible.max_y() + CARD_SHADOW_PADDING - card.1 - 24.0,
        ),
    };
    (
        clamp(
            origin.0,
            visible.x - CARD_SHADOW_PADDING,
            visible.max_x() + CARD_SHADOW_PADDING - card.0,
        ),
        clamp(
            origin.1,
            visible.y - CARD_SHADOW_PADDING,
            visible.max_y() + CARD_SHADOW_PADDING - card.1,
        ),
    )
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.clamp(low, high.max(low))
}

pub fn follow_target(
    settings: Option<Rect>,
    card: (f64, f64),
    visible: Rect,
    current: (f64, f64),
    frozen: bool,
) -> Option<(f64, f64)> {
    if frozen {
        return None;
    }
    let settings = settings?;
    let target = perch(Some(settings), card, visible);
    if (target.0 - current.0).abs() <= FOLLOW_EPSILON
        && (target.1 - current.1).abs() <= FOLLOW_EPSILON
    {
        return None;
    }
    Some(target)
}

pub fn screen_visible_for(screens: &[(Rect, Rect)], settings: Rect) -> Option<Rect> {
    screens
        .iter()
        .find(|(frame, _)| frame.intersects(settings))
        .map(|(_, visible)| *visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: (f64, f64) = (460.0, 140.0);

    fn main_visible() -> Rect {
        Rect::new(0.0, 0.0, 1440.0, 900.0)
    }

    fn settings_at(x: f64) -> Rect {
        Rect::new(x, 100.0, 800.0, 600.0)
    }

    #[test]
    fn rect_geometry() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);

        assert_eq!(rect.max_x(), 40.0);
        assert_eq!(rect.max_y(), 60.0);
        assert_eq!(rect.area(), 1200.0);
        assert!(rect.contains(10.0, 20.0));
        assert!(!rect.contains(40.0, 60.0));
        assert!(rect.contains(39.999, 59.999));
        assert!(!rect.contains(9.999, 20.0));
    }

    #[test]
    fn rect_intersects() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);

        assert!(rect.intersects(Rect::new(5.0, 5.0, 10.0, 10.0)));
        assert!(!rect.intersects(Rect::new(20.0, 20.0, 5.0, 5.0)));
    }

    #[test]
    fn perch_places_inside_bottom_right() {
        let settings = settings_at(100.0);

        let placement = perch(Some(settings), CARD, main_visible());

        assert_eq!(placement, (446.0, 566.0));
        assert_eq!(
            placement.1 + CARD.1 - CARD_SHADOW_PADDING,
            settings.max_y() - CARD_INSET
        );
        assert_eq!(
            placement.0 + CARD.0 - CARD_SHADOW_PADDING,
            settings.max_x() - CARD_INSET
        );
    }

    #[test]
    fn perch_without_settings_uses_fallback_corner() {
        let visible = main_visible();

        let placement = perch(None, CARD, visible);

        assert_eq!(placement, (980.0, 760.0));
        assert_eq!(
            placement.1 + CARD.1 - CARD_SHADOW_PADDING,
            visible.max_y() - 24.0
        );
        assert_eq!(
            placement.0 + CARD.0 - CARD_SHADOW_PADDING,
            visible.max_x() - 24.0
        );
    }

    #[test]
    fn perch_clamps_to_visible_when_settings_offscreen_right() {
        let settings = Rect::new(900.0, 100.0, 600.0, 600.0);

        let placement = perch(Some(settings), CARD, main_visible());

        assert_eq!(placement.0, 1004.0);
        assert_eq!(placement.1, 566.0);
    }

    #[test]
    fn perch_clamps_when_settings_below_visible() {
        let settings = Rect::new(100.0, 800.0, 800.0, 600.0);

        let placement = perch(Some(settings), CARD, main_visible());

        assert_eq!(placement.1, 784.0);
    }

    #[test]
    fn follow_ignores_subpixel_moves() {
        let current = (446.5, 565.5);

        let moved = follow_target(
            Some(settings_at(100.0)),
            CARD,
            main_visible(),
            current,
            false,
        );

        assert_eq!(moved, None);
    }

    #[test]
    fn follow_moves_when_settings_moves() {
        let current = (446.0, 566.0);

        let moved = follow_target(
            Some(settings_at(140.0)),
            CARD,
            main_visible(),
            current,
            false,
        );

        assert_eq!(moved, Some((486.0, 566.0)));
        assert_eq!(moved.map(|origin| origin.0 - current.0), Some(40.0));
    }

    #[test]
    fn follow_stays_when_settings_missing() {
        let moved = follow_target(None, CARD, main_visible(), (300.0, 200.0), false);

        assert_eq!(moved, None);
    }

    #[test]
    fn follow_frozen_during_drag() {
        let moved = follow_target(
            Some(settings_at(140.0)),
            CARD,
            main_visible(),
            (446.0, 566.0),
            true,
        );

        assert_eq!(moved, None);
    }

    #[test]
    fn follow_resumes_after_freeze() {
        let moved = follow_target(
            Some(settings_at(140.0)),
            CARD,
            main_visible(),
            (446.0, 566.0),
            false,
        );

        assert_eq!(moved, Some((486.0, 566.0)));
    }

    #[test]
    fn screen_visible_for_picks_intersecting_screen() {
        let primary_frame = Rect::new(0.0, 0.0, 1440.0, 900.0);
        let primary_visible = Rect::new(0.0, 25.0, 1440.0, 850.0);
        let secondary_frame = Rect::new(1440.0, 0.0, 1920.0, 1080.0);
        let secondary_visible = Rect::new(1440.0, 25.0, 1920.0, 1030.0);
        let screens = [
            (primary_frame, primary_visible),
            (secondary_frame, secondary_visible),
        ];

        let picked = screen_visible_for(&screens, Rect::new(1600.0, 200.0, 400.0, 300.0));
        assert_eq!(picked, Some(secondary_visible));

        let missing = screen_visible_for(&screens, Rect::new(4000.0, 4000.0, 100.0, 100.0));
        assert_eq!(missing, None);
    }
}
