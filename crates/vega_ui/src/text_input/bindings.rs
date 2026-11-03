use super::*;

impl TextInput {
    pub(super) fn accessible_label(&self) -> String {
        let placeholder = self.localized_placeholder();
        let mentions = self.skill_mentions();
        if self.masked || mentions.is_empty() {
            return placeholder.to_string();
        }
        let names = mentions
            .iter()
            .map(|mention| {
                if mention.text.contains(&mention.intent.name) {
                    mention.text.clone()
                } else {
                    format!("{} ({})", mention.text, mention.intent.name)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{placeholder}; {names}")
    }

    pub fn skill_mentions(&self) -> Vec<SkillMention> {
        self.skill_mentions
            .iter()
            .filter(|mention| {
                mention.range.start < mention.range.end
                    && self.content.get(mention.range.clone()) == Some(mention.text.as_str())
            })
            .cloned()
            .collect()
    }

    pub fn set_text_with_bindings(
        &mut self,
        text: &str,
        references: &[FileReferenceMention],
        skills: &[SkillMention],
        cx: &mut Context<Self>,
    ) {
        self.set_text(text, cx);
        let mut references = references.to_vec();
        references.sort_by_key(|mention| mention.range.start);
        let mut cursor = 0;
        for mention in references {
            if mention.range.start < cursor
                || mention.range.start >= mention.range.end
                || text.get(mention.range.clone()) != Some(format!("@{}", mention.path).as_str())
            {
                continue;
            }
            cursor = mention.range.end;
            self.mentions
                .push(Mention::new(mention.range, mention.path));
        }
        let mut skills = skills.to_vec();
        skills.sort_by_key(|mention| mention.range.start);
        cursor = 0;
        for mention in skills {
            if mention.range.start < cursor
                || mention.range.start >= mention.range.end
                || text.get(mention.range.clone()) != Some(mention.text.as_str())
                || self.mentions.iter().any(|reference| {
                    reference.range.start < mention.range.end
                        && mention.range.start < reference.range.end
                })
            {
                continue;
            }
            cursor = mention.range.end;
            self.skill_mentions.push(mention);
        }
        cx.notify();
    }

    pub fn replace_composer_token(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        cx: &mut Context<Self>,
    ) {
        if self.is_composing() || self.content.get(range.clone()).is_none() {
            return;
        }
        self.record_edit();
        self.apply_edit(range, replacement);
        cx.notify();
    }

    pub fn insert_skill_mention(
        &mut self,
        range: Range<usize>,
        text: &str,
        intent: SkillSelectionIntent,
        cx: &mut Context<Self>,
    ) {
        if self.is_composing()
            || text.is_empty()
            || text.trim_end() != text
            || self.content.get(range.clone()).is_none()
        {
            return;
        }
        self.record_edit();
        let start = range.start;
        self.apply_edit(range, &format!("{text} "));
        let mention = SkillMention {
            range: start..start + text.len(),
            text: text.to_owned(),
            intent,
        };
        let position = self
            .skill_mentions
            .partition_point(|existing| existing.range.start < start);
        self.skill_mentions.insert(position, mention);
        cx.notify();
    }

    pub(super) fn apply_edit(&mut self, range: Range<usize>, replacement: &str) {
        self.prediction = None;
        self.content_revision = self.content_revision.wrapping_add(1);
        self.content =
            (self.content[..range.start].to_owned() + replacement + &self.content[range.end..])
                .into();
        let caret = range.start + replacement.len();
        self.remap_mentions(range, replacement.len());
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        self.mouse_selection = None;
        self.vertical_goal = None;
        self.editor_scroll = None;
        self.update_logical_viewport();
    }
}

pub(super) fn remap_binding(
    range: &mut Range<usize>,
    edited: &Range<usize>,
    inserted: usize,
) -> bool {
    let insertion = edited.is_empty();
    let overlaps = if insertion {
        range.start < edited.start && edited.start < range.end
    } else {
        edited.start < range.end && range.start < edited.end
    };
    if overlaps {
        return false;
    }
    if edited.end <= range.start {
        let removed = edited.end - edited.start;
        range.start = range.start - removed + inserted;
        range.end = range.end - removed + inserted;
    }
    true
}
