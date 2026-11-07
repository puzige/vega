CREATE TABLE preparation_revisions (
    thread_id TEXT PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>=0)
) STRICT;
INSERT INTO preparation_revisions SELECT id,0 FROM threads;

CREATE TRIGGER preparation_revision_insert BEFORE INSERT ON preparation_revisions
BEGIN
    SELECT RAISE(ABORT,'preparation revision missing') WHERE EXISTS(SELECT 1 FROM threads WHERE id=NEW.thread_id) AND NOT EXISTS(SELECT 1 FROM preparation_revisions WHERE thread_id=NEW.thread_id);
END;

CREATE TRIGGER preparation_revision_update BEFORE UPDATE ON preparation_revisions
BEGIN
    SELECT RAISE(ABORT,'preparation revision unavailable') WHERE NEW.thread_id IS NOT OLD.thread_id OR typeof(NEW.revision)<>'integer' OR NEW.revision<=OLD.revision;
END;

CREATE TRIGGER preparation_revision_delete BEFORE DELETE ON preparation_revisions
BEGIN
    SELECT RAISE(ABORT,'preparation revision tombstone required');
END;

CREATE TRIGGER preparation_threads_insert BEFORE INSERT ON threads
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.id AS owner FROM threads victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_threads_update BEFORE UPDATE ON threads
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.id AS owner FROM threads victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_threads_delete BEFORE DELETE ON threads
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_messages_insert BEFORE INSERT ON messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM messages victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_messages_update BEFORE UPDATE ON messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM messages victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_messages_delete BEFORE DELETE ON messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_calls_insert BEFORE INSERT ON tool_calls
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=NEW.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM tool_calls victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM tool_calls victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_calls_update BEFORE UPDATE ON tool_calls
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=OLD.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=NEW.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM tool_calls victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM tool_calls victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_calls_delete BEFORE DELETE ON tool_calls
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=OLD.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_image_attachments_insert BEFORE INSERT ON image_attachments
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM image_attachments victim WHERE (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_image_attachments_update BEFORE UPDATE ON image_attachments
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM image_attachments victim WHERE (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_image_attachments_delete BEFORE DELETE ON image_attachments
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_pending_messages_insert BEFORE INSERT ON pending_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM pending_messages victim WHERE (victim.id IS NEW.id) OR (victim.thread_id IS NEW.thread_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_pending_messages_update BEFORE UPDATE ON pending_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM pending_messages victim WHERE (victim.id IS NEW.id) OR (victim.thread_id IS NEW.thread_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_pending_messages_delete BEFORE DELETE ON pending_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_message_delivery_insert BEFORE INSERT ON historical_message_delivery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=NEW.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM historical_message_delivery victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM historical_message_delivery victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_message_delivery_update BEFORE UPDATE ON historical_message_delivery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=OLD.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=NEW.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM historical_message_delivery victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM historical_message_delivery victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_message_delivery_delete BEFORE DELETE ON historical_message_delivery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM messages WHERE id=OLD.message_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_tool_receipts_insert BEFORE INSERT ON historical_tool_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM historical_tool_receipts victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM historical_tool_receipts victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_tool_receipts_update BEFORE UPDATE ON historical_tool_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM historical_tool_receipts victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM historical_tool_receipts victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_historical_tool_receipts_delete BEFORE DELETE ON historical_tool_receipts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_call_rounds_insert BEFORE INSERT ON tool_call_rounds
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM tool_call_rounds victim WHERE (victim.message_id IS NEW.message_id AND victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_call_rounds_update BEFORE UPDATE ON tool_call_rounds
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM tool_call_rounds victim WHERE (victim.message_id IS NEW.message_id AND victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_tool_call_rounds_delete BEFORE DELETE ON tool_call_rounds
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_micro_compactions_insert BEFORE INSERT ON context_micro_compactions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_micro_compactions victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM context_micro_compactions victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_micro_compactions_update BEFORE UPDATE ON context_micro_compactions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_micro_compactions victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM context_micro_compactions victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_micro_compactions_delete BEFORE DELETE ON context_micro_compactions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_computer_screenshot_artifacts_insert BEFORE INSERT ON computer_screenshot_artifacts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM computer_screenshot_artifacts victim WHERE (victim.call_id IS NEW.call_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM computer_screenshot_artifacts victim WHERE (victim.call_id IS NEW.call_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_computer_screenshot_artifacts_update BEFORE UPDATE ON computer_screenshot_artifacts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner UNION SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM computer_screenshot_artifacts victim WHERE (victim.call_id IS NEW.call_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM computer_screenshot_artifacts victim WHERE (victim.call_id IS NEW.call_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_computer_screenshot_artifacts_delete BEFORE DELETE ON computer_screenshot_artifacts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner UNION SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_checkpoints_insert BEFORE INSERT ON context_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_checkpoints victim WHERE (victim.id IS NEW.id) OR (victim.thread_id IS NEW.thread_id AND victim.model IS NEW.model AND victim.source_version IS NEW.source_version AND victim.source_fingerprint IS NEW.source_fingerprint)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_checkpoints_update BEFORE UPDATE ON context_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM context_checkpoints victim WHERE (victim.id IS NEW.id) OR (victim.thread_id IS NEW.thread_id AND victim.model IS NEW.model AND victim.source_version IS NEW.source_version AND victim.source_fingerprint IS NEW.source_fingerprint)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_context_checkpoints_delete BEFORE DELETE ON context_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_responses_insert BEFORE INSERT ON assistant_responses
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM assistant_responses victim WHERE (victim.message_id IS NEW.message_id AND victim.response_id IS NEW.response_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_responses_update BEFORE UPDATE ON assistant_responses
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM assistant_responses victim WHERE (victim.message_id IS NEW.message_id AND victim.response_id IS NEW.response_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_responses_delete BEFORE DELETE ON assistant_responses
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_response_starts_insert BEFORE INSERT ON assistant_response_starts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM assistant_response_starts victim WHERE (victim.message_id IS NEW.message_id AND victim.response_id IS NEW.response_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_response_starts_update BEFORE UPDATE ON assistant_response_starts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM assistant_response_starts victim WHERE (victim.message_id IS NEW.message_id AND victim.response_id IS NEW.response_id) OR (victim.message_id IS NEW.message_id AND victim.ordinal IS NEW.ordinal)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_assistant_response_starts_delete BEFORE DELETE ON assistant_response_starts
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_message_timestamp_boundaries_insert BEFORE INSERT ON message_timestamp_boundaries
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM message_timestamp_boundaries victim WHERE (victim.message_id IS NEW.message_id AND victim.operation_key IS NEW.operation_key)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_message_timestamp_boundaries_update BEFORE UPDATE ON message_timestamp_boundaries
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM message_timestamp_boundaries victim WHERE (victim.message_id IS NEW.message_id AND victim.operation_key IS NEW.operation_key)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_message_timestamp_boundaries_delete BEFORE DELETE ON message_timestamp_boundaries
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_generated_notices_insert BEFORE INSERT ON generated_notices
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM generated_notices victim WHERE (victim.message_id IS NEW.message_id AND victim.text_start_bytes IS NEW.text_start_bytes)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_generated_notices_update BEFORE UPDATE ON generated_notices
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=NEW.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=victim.message_id) AS owner FROM generated_notices victim WHERE (victim.message_id IS NEW.message_id AND victim.text_start_bytes IS NEW.text_start_bytes)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_generated_notices_delete BEFORE DELETE ON generated_notices
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM messages WHERE id=OLD.message_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_executions_insert BEFORE INSERT ON delegated_executions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_executions victim WHERE (victim.child_message_id IS NEW.child_message_id) OR (victim.child_thread_id IS NEW.child_thread_id AND victim.ordinal IS NEW.ordinal) OR (victim.parent_thread_id IS NEW.parent_thread_id AND victim.parent_message_id IS NEW.parent_message_id AND victim.request_call_id IS NEW.request_call_id) OR (victim.child_thread_id IS NEW.child_thread_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_executions_update BEFORE UPDATE ON delegated_executions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_executions victim WHERE (victim.child_message_id IS NEW.child_message_id) OR (victim.child_thread_id IS NEW.child_thread_id AND victim.ordinal IS NEW.ordinal) OR (victim.parent_thread_id IS NEW.parent_thread_id AND victim.parent_message_id IS NEW.parent_message_id AND victim.request_call_id IS NEW.request_call_id) OR (victim.child_thread_id IS NEW.child_thread_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_executions_delete BEFORE DELETE ON delegated_executions
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_messages_insert BEFORE INSERT ON delegated_execution_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_execution_messages victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_messages_update BEFORE UPDATE ON delegated_execution_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_execution_messages victim WHERE (victim.message_id IS NEW.message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_messages_delete BEFORE DELETE ON delegated_execution_messages
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_recovery_insert BEFORE INSERT ON delegated_execution_recovery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_execution_recovery victim WHERE (victim.child_thread_id IS NEW.child_thread_id AND victim.child_message_id IS NEW.child_message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_recovery_update BEFORE UPDATE ON delegated_execution_recovery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_execution_recovery victim WHERE (victim.child_thread_id IS NEW.child_thread_id AND victim.child_message_id IS NEW.child_message_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_execution_recovery_delete BEFORE DELETE ON delegated_execution_recovery
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_stage_checkpoints_insert BEFORE INSERT ON delegated_stage_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_stage_checkpoints victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_stage_checkpoints_update BEFORE UPDATE ON delegated_stage_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.child_thread_id AS owner FROM delegated_stage_checkpoints victim WHERE (victim.id IS NEW.id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_delegated_stage_checkpoints_delete BEFORE DELETE ON delegated_stage_checkpoints
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.child_thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_fork_mutation_origins_insert BEFORE INSERT ON fork_mutation_origins
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM fork_mutation_origins victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_fork_mutation_origins_update BEFORE UPDATE ON fork_mutation_origins
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=NEW.call_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=victim.call_id) AS owner FROM fork_mutation_origins victim WHERE (victim.call_id IS NEW.call_id)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_fork_mutation_origins_delete BEFORE DELETE ON fork_mutation_origins
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT (SELECT thread_id FROM tool_calls WHERE id=OLD.call_id) AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_thread_timestamp_fork_prefixes_insert BEFORE INSERT ON thread_timestamp_fork_prefixes
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM thread_timestamp_fork_prefixes victim WHERE (victim.thread_id IS NEW.thread_id AND victim.prefix_seq IS NEW.prefix_seq)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_thread_timestamp_fork_prefixes_update BEFORE UPDATE ON thread_timestamp_fork_prefixes
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT NEW.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT victim.thread_id AS owner FROM thread_timestamp_fork_prefixes victim WHERE (victim.thread_id IS NEW.thread_id AND victim.prefix_seq IS NEW.prefix_seq)) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

CREATE TRIGGER preparation_thread_timestamp_fork_prefixes_delete BEFORE DELETE ON thread_timestamp_fork_prefixes
BEGIN
    INSERT INTO preparation_revisions(thread_id,revision)
    SELECT owner,1 FROM (SELECT OLD.thread_id AS owner) WHERE owner IS NOT NULL
    ON CONFLICT(thread_id) DO UPDATE SET revision=revision+1;
END;

