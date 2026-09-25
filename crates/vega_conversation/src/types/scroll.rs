#[derive(Clone, Debug, PartialEq)]
pub struct ThreadScrollAnchor {
    pub identity: Option<String>,
    pub message_id: Option<String>,
    pub offset_in_item_px: f32,
    pub following_tail: bool,
}
