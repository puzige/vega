CREATE TABLE context_precompute_accounting (
    generation TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    model TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0 CHECK (retired IN (0, 1)),
    stage_count INTEGER CHECK (stage_count BETWEEN 0 AND 32),
    usage_complete INTEGER CHECK (usage_complete IN (0, 1)),
    CHECK ((stage_count IS NULL) = (usage_complete IS NULL))
);
CREATE TABLE context_precompute_stages (
    generation TEXT NOT NULL REFERENCES context_precompute_accounting(generation),
    stage INTEGER NOT NULL CHECK (stage BETWEEN 1 AND 32),
    payload TEXT NOT NULL CHECK (length(CAST(payload AS BLOB)) <= 16384),
    usage_id INTEGER REFERENCES token_usage(id),
    PRIMARY KEY (generation, stage)
);
CREATE TABLE context_precompute_slots (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    generation TEXT NOT NULL UNIQUE REFERENCES context_precompute_accounting(generation)
);
CREATE TABLE context_precompute_ready (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    generation TEXT NOT NULL UNIQUE REFERENCES context_precompute_accounting(generation),
    payload TEXT NOT NULL CHECK (length(CAST(payload AS BLOB)) <= 8000000)
);
