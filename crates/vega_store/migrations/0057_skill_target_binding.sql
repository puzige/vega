ALTER TABLE skill_approvals ADD COLUMN target_binding TEXT
    CHECK (target_binding IS NULL OR (length(target_binding) <= 8192 AND json_valid(target_binding)));
