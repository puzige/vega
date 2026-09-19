-- Issue 65 R4/R5: title-only provenance prevents manual-rename ABA races.
ALTER TABLE threads ADD COLUMN auto_title_state TEXT NOT NULL DEFAULT 'eligible'
    CHECK (auto_title_state IN ('eligible', 'claimed', 'generated', 'manual', 'legacy'));
ALTER TABLE threads ADD COLUMN auto_title_claim TEXT;
UPDATE threads SET auto_title_state = 'legacy'
WHERE title != '' OR EXISTS (SELECT 1 FROM messages WHERE messages.thread_id = threads.id);
