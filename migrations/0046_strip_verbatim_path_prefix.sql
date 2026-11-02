-- 0046_strip_verbatim_path_prefix: 清洗 work_directories.abs_path 的 Windows
-- verbatim 路径前缀（\\?\ 与 \\?\UNC\），使显示路径为 DOS 风格。
-- 配合代码层 dunce::canonicalize（新写入已不带前缀）一次性修正存量数据。
-- Append-only — never edit shipped migrations.
--
-- 背景：std::fs::canonicalize 在 Windows 上对已存在路径返回带 \\?\ 前缀的
-- verbatim 路径（Win32 设备命名空间，用于绕过 MAX_PATH 260 字符限制）。
-- 该前缀被原样存入 abs_path 并显示给用户（见 K-display fix）。代码层已改用
-- dunce::canonicalize 在安全路径下剥离前缀；本 migration 修正已落库的存量。
--
-- 顺序敏感：先剥 UNC 前缀（\\?\UNC\ 更长），再剥盘符前缀（\\?\）。
-- 若先剥盘符，UNC 路径 \\?\UNC\server\share 会被误剥成 UNC\server\share。

-- 1) UNC verbatim 前缀 \\?\UNC\server\share\... → \\server\share\...
--    前缀 \\?\UNC\ 为 8 字符；跳过 8 字符，前置 \\ 还原 UNC 形式。
UPDATE work_directories
SET abs_path = '\\' || substr(abs_path, 9)
WHERE substr(abs_path, 1, 8) = '\\?\UNC\';

-- 2) 盘符 verbatim 前缀 \\?\C:\... → C:\...
--    跳过前 4 个字符（\\?\）。
UPDATE work_directories
SET abs_path = substr(abs_path, 5)
WHERE substr(abs_path, 1, 4) = '\\?\';
