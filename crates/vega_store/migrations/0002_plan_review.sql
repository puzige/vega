ALTER TABLE messages ADD COLUMN plan_status TEXT
    CHECK (
        plan_status IS NULL OR plan_status IN (
            'pending',
            'approved',
            'changes_requested',
            'abandoned'
        )
    );

ALTER TABLE messages ADD COLUMN plan_review_note TEXT;

ALTER TABLE messages ADD COLUMN plan_reviewed_at INTEGER;
