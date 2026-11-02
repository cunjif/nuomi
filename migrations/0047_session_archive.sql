-- 0047_session_archive: 会话归档（删除工作区时归档其会话，保留路径快照供恢复匹配）。
--   archived_at              归档时间戳（unix-ms）；NULL = 未归档。归档会话从正常列表隐藏，
--                            在「全部」过滤的归档入口中可见，可彻底删除或按路径匹配恢复。
--   archived_workspace_path  归档时原工作区 root_path 快照（恢复时按路径匹配现有工作区）。
--   archived_directory_path  归档时原工作目录 abs_path 快照（恢复时按路径匹配现有挂载目录）。
ALTER TABLE sessions ADD COLUMN archived_at INTEGER;
ALTER TABLE sessions ADD COLUMN archived_workspace_path TEXT;
ALTER TABLE sessions ADD COLUMN archived_directory_path TEXT;
CREATE INDEX idx_sessions_archived ON sessions (archived_at) WHERE archived_at IS NOT NULL;
