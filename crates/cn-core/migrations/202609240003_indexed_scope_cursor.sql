-- Reconcile stale indexed scopes in finite pages across worker restarts.
CREATE TABLE cn_index.indexed_scope_cursor (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    last_kind TEXT NOT NULL DEFAULT '',
    last_scope_id TEXT NOT NULL DEFAULT ''
);
INSERT INTO cn_index.indexed_scope_cursor (id) VALUES (TRUE);
