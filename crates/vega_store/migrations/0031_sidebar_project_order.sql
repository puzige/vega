WITH missing_projects AS MATERIALIZED (
    SELECT
        projects.id,
        (SELECT COALESCE(MAX(position), -1) FROM sidebar_project_order)
            + ROW_NUMBER() OVER (ORDER BY projects.last_opened_at DESC, projects.id ASC) AS position
    FROM projects
    WHERE NOT EXISTS (
        SELECT 1 FROM sidebar_project_order
        WHERE project_id = projects.id
    )
)
INSERT INTO sidebar_project_order(project_id, position)
SELECT id, position FROM missing_projects ORDER BY position, id;

CREATE TRIGGER sidebar_project_order_append
AFTER INSERT ON projects
BEGIN
    INSERT INTO sidebar_project_order(project_id, position)
    SELECT NEW.id, COALESCE(MAX(position), -1) + 1
    FROM sidebar_project_order;
END;
