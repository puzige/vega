WITH legacy AS (
    SELECT preferences, json_extract(preferences, '$.sort') AS sort
    FROM sidebar_organization
    WHERE singleton = 1
), ranks AS (
    SELECT value AS id, MAX(CAST(key AS INTEGER)) AS rank
    FROM legacy, json_each(legacy.preferences, '$.manual_thread_order')
    WHERE type = 'text'
    GROUP BY value
), ordered AS (
    SELECT threads.id
    FROM threads
    CROSS JOIN legacy
    LEFT JOIN ranks ON ranks.id = threads.id
    WHERE threads.parent_thread_id IS NULL OR threads.pinned = 1
    ORDER BY
        CASE WHEN legacy.sort = 'Manual' AND ranks.rank IS NOT NULL THEN 1 ELSE 0 END,
        CASE WHEN legacy.sort = 'Manual' THEN ranks.rank END,
        CASE WHEN legacy.sort <> 'Manual' THEN threads.pinned END DESC,
        CASE WHEN legacy.sort = 'Created' THEN threads.created_at ELSE threads.updated_at END DESC,
        threads.id
)
UPDATE sidebar_organization
SET preferences = json_set(
    preferences,
    '$.sort', 'Manual',
    '$.manual_thread_order', json((SELECT json_group_array(id) FROM ordered))
)
WHERE singleton = 1;
