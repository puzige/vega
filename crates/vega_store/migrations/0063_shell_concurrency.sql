ALTER TABLE shell_operations ADD COLUMN concurrent INTEGER NOT NULL DEFAULT 0 CHECK (concurrent IN (0, 1));
