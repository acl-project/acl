# SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
# SPDX-License-Identifier: GPL-3.0-or-later

# Source: The account with UUID '{ $uuid }' does not exist
auth-error-account-not-found = UUID 为 '{ $uuid }' 的账户不存在
# Source: Username cannot be empty
auth-error-empty-username = 用户名不能为空
# Source: Network request failed: { $arg0 }
auth-error-http = 网络请求失败：{ $arg0 }
# Source: Invalid OAuth callback request: { $reason }
auth-error-invalid-callback-request = 无效的 OAuth 回调请求：{ $reason }
# Source: Username contains invalid character '{ $ch }'; only ASCII alphanumeric characters and underscores are allowed
auth-error-invalid-username-char = 用户名包含无效字符 '{ $ch }'；仅允许 ASCII 字母数字和下划线
# Source: Username length must be between 3 and 16 characters, got { $len }
auth-error-invalid-username-length = 用户名长度必须在 3 到 16 个字符之间，实际为 { $len }
# Source: I/O error at path '{ $path }': { $source }
auth-error-io = 路径 '{ $path }' 处发生 I/O 错误：{ $source }
# Source: Failed to parse JSON: { $arg0 }
auth-error-json = 解析 JSON 失败：{ $arg0 }
# Source: Keyring error: { $arg0 }
auth-error-keyring = 密钥环错误：{ $arg0 }
# Source: OAuth callback returned error '{ $error }': { $description }
auth-error-o-auth-callback-error = OAuth 回调返回错误 '{ $error }'：{ $description }
# Source: OAuth login timed out waiting for browser callback
auth-error-o-auth-timeout = OAuth 登录等待浏览器回调超时
# Source: Microsoft refresh token is expired
auth-error-refresh-token-expired = Microsoft 刷新令牌已过期
# Source: Socket I/O error: { $arg0 }
auth-error-socket = Socket I/O 错误：{ $arg0 }
# Source: OAuth state mismatch
auth-error-state-mismatch = OAuth state 不匹配
# Source: Microsoft OAuth token request failed ({ $status }): { $message }
auth-error-token-request-failed = Microsoft OAuth 令牌请求失败（{ $status }）：{ $message }
# Source: Failed to parse URL: { $arg0 }
auth-error-url-parse = 解析 URL 失败：{ $arg0 }
# Source: Uuid error: { $arg0 }
auth-error-uuid = UUID 错误：{ $arg0 }
# Source: Failed to parse configuration file: { $arg0 }
core-error-config-parse = 解析配置文件失败：{ $arg0 }
# Source: Failed to serialize configuration: { $arg0 }
core-error-config-serialize = 序列化配置失败：{ $arg0 }
# Source: Instance '{ $id }' already exists
core-error-instance-already-exists = 实例 '{ $id }' 已存在
# Source: Instance '{ $id }' not found
core-error-instance-not-found = 未找到实例 '{ $id }'
# Source: Invalid instance id '{ $id }': { $reason }
core-error-invalid-instance-id = 无效的实例 ID '{ $id }'：{ $reason }
# Source: Invalid memory configuration: min { $min_mb } MB cannot exceed max { $max_mb } MB
core-error-invalid-memory-config = 无效的内存配置：最小值 { $min_mb } MB 不能超过最大值 { $max_mb } MB
# Source: I/O error at path '{ $path }': { $source }
core-error-io = 路径 '{ $path }' 处发生 I/O 错误：{ $source }
# Source: Failed to find any Java runtime
core-error-java-not-found = 未找到任何 Java 运行时
# Source: Failed to probe Java version: { $path }: { $reason }
core-error-java-probe-failed = 探测 Java 版本失败：{ $path }：{ $reason }
# Source: Java version mismatch: required { $required_major }, found { $found_major }, path: { $path }
core-error-java-version-mismatch = Java 版本不匹配：需要 { $required_major }，找到 { $found_major }，路径：{ $path }
# Source: Failed to join classpath entries: { $arg0 }
core-error-join-paths = 拼接 classpath 条目失败：{ $arg0 }
# Source: Failed to parse JSON: { $arg0 }
core-error-json = 解析 JSON 失败：{ $arg0 }
# Source: Minecraft error: { $arg0 }
core-error-minecraft = Minecraft 错误：{ $arg0 }
# Source: Failed to spawn game process with Java '{ $java_path }': { $source }
core-error-process-spawn = 使用 Java '{ $java_path }' 启动游戏进程失败：{ $source }
# Source: Failed to determine standard application directories
core-error-project-dirs-unavailable = 无法确定标准应用目录
# Source: System clock error: { $arg0 }
core-error-system-time = 系统时钟错误：{ $arg0 }
# Source: SHA-1 mismatch for '{ $target }': expected { $expected }, actual { $actual }
minecraft-error-hash-mismatch = '{ $target }' 的 SHA-1 不匹配：期望 { $expected }，实际 { $actual }
# Source: HTTP request failed: { $arg0 }
minecraft-error-http = HTTP 请求失败：{ $arg0 }
# Source: I/O error at path '{ $path }': { $source }
minecraft-error-io = 路径 '{ $path }' 处发生 I/O 错误：{ $source }
# Source: Failed to parse JSON: { $arg0 }
minecraft-error-json = 解析 JSON 失败：{ $arg0 }
# Source: System clock error: { $arg0 }
minecraft-error-system-time = 系统时钟错误：{ $arg0 }
# Source: Minecraft version '{ $version_id }' not found in manifest
minecraft-error-version-not-found = 清单中未找到 Minecraft 版本 '{ $version_id }'
# Source: Zip archive error at path '{ $path }': { $source }
minecraft-error-zip = 路径 '{ $path }' 处的 Zip 归档错误：{ $source }
