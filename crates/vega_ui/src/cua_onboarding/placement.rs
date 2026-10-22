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
