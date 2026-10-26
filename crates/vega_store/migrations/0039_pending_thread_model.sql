ALTER TABLE threads ADD COLUMN pending_model TEXT CHECK (pending_model IS NULL OR length(pending_model) > 0);
