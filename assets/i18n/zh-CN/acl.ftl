# SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
# SPDX-License-Identifier: GPL-3.0-or-later

# Source: Microsoft
account-type-microsoft = 微软账号
# Source: Offline
account-type-offline = 离线账号
# Source: Are you sure you want to delete account "{ $username }"?
accounts-delete-dialog-body = 确定要删除账户“{ $username }”吗？
# Source: Cancel
accounts-delete-dialog-cancel = 取消
# Source: Delete
accounts-delete-dialog-delete = 删除
# Source: Delete Account
accounts-delete-dialog-heading = 删除账户
# Source: Invalid username: { $err }
accounts-error-invalid-username = 无效的用户名：{ $err }
# Source: Failed to open browser: { $err }
accounts-error-open-browser = 打开浏览器失败：{ $err }
# Source: Failed to remove account: { $err }
accounts-error-remove-account = 移除账户失败：{ $err }
# Source: Failed to remove tokens: { $err }
accounts-error-remove-tokens = 移除令牌失败：{ $err }
# Source: Failed to save accounts: { $err }
accounts-error-save-accounts = 保存账户失败：{ $err }
# Source: Failed to select account: { $err }
accounts-error-select-account = 选择账户失败：{ $err }
# Source: Add Offline
accounts-group-button-add-offline = 添加离线
# Source: Microsoft Login
accounts-group-button-microsoft = 微软登录
# Source: Select an account to launch Minecraft
accounts-group-description = 选择一个用于启动 Minecraft 的账户
# Source: Accounts
accounts-group-title = 账户
# Source: Add an offline account
accounts-group-tooltip-add-offline = 添加离线账户
# Source: Sign in with a Microsoft account
accounts-group-tooltip-sign-in-microsoft = 使用微软账户登录
# Source: Accounts
accounts-page-title = 账户
# Source: Remove account
accounts-tooltip-remove = 移除账户
# Source: Add
add-offline-dialog-add = 添加
# Source: Enter a Minecraft username (3-16 characters, letters, numbers, and underscores).
add-offline-dialog-body = 请输入 Minecraft 用户名（3 到 16 个字符，仅支持字母、数字和下划线）。
# Source: Cancel
add-offline-dialog-cancel = 取消
# Source: Add Offline Account
add-offline-dialog-heading = 添加离线账户
# Source: Failed to load accounts, using defaults: { $err }
app-init-error-load-accounts = 加载账户失败，使用默认配置：{ $err }
# Source: Failed to load config, using defaults: { $err }
app-init-error-load-config = 加载配置失败，使用默认配置：{ $err }
# Source: Failed to load instances, using defaults: { $err }
app-init-error-load-instances = 加载实例失败，使用默认配置：{ $err }
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
# Source: Are you sure you want to delete instance "{ $name }"?
delete-dialog-body = 确定要删除实例“{ $name }”吗？
# Source: Cancel
delete-dialog-cancel = 取消
# Source: Delete
delete-dialog-delete = 删除
# Source: Delete Instance
delete-dialog-heading = 删除实例
# Source: Launching...
home-page-button-launching = 正在启动...
# Source: Start Game
home-page-button-start-game = 启动游戏
# Source: Please select a instance
home-page-error-empty-instance = 请先选择一个实例
# Source: Failed to save accounts: { $err }
home-page-error-save-accounts = 保存账户失败：{ $err }
# Source: No account selected
home-page-no-account = 未选择账户
# Source: No instance selected
home-page-no-instance = 未选择实例
# Source: Instance: { $inst_text } • Account: { $acc_text }
home-page-status-description = 实例：{ $inst_text } • 账户：{ $acc_text }
# Source: Adwaita Craft Launcher
home-page-status-title = Adwaita Craft Launcher
# Source: Instance settings
home-page-tooltip-instance-settings = 实例设置
# Source: Downloading ({ $completed }/{ $total })
install-stage-downloading = 正在下载 ({ $completed }/{ $total })
# Source: Fetching asset index...
install-stage-fetching-asset-index = 正在获取资源索引...
# Source: Fetching metadata...
install-stage-fetching-metadata = 正在获取版本元数据...
# Source: Preparing runtime...
install-stage-preparing-runtime = 正在准备运行环境...
# Source: Back
instance-detail-button-back = 返回
# Source: Failed to remove instance: { $err }
instance-list-error-remove-instance = 移除实例失败：{ $err }
# Source: Failed to select instance: { $err }
instance-list-error-select-instance = 选择实例失败：{ $err }
# Source: Instance "{ $id }" is ready to play
instance-list-ready-to-play = 实例“{ $id }”已准备就绪
# Source: Settings
instance-navigation-page-settings = 设置
# Source: Base
instance-settings-base-title = 基础
# Source: Instance-Specific Settings
instance-settings-custom-title = 独立实例设置
# Source: Failed to save instance { $id } configuration: { $err }
instance-settings-error-save-configuration = 保存实例 { $id } 配置失败：{ $err }
# Source: Instance Name
instance-settings-name-title = 实例名称
# Source: None
instance-settings-none = 无
# Source: Back
instances-create-button-back = 返回
# Source: Create
instances-create-button-create = 创建
# Source: Failed to create instance: { $err }
instances-create-error-create-instance = 创建实例失败：{ $err }
# Source: Please select a Minecraft version
instances-create-error-empty-version = 请选择 Minecraft 版本
# Source: Failed to load version manifest: { $err }
instances-create-error-load-version-manifest = 加载版本清单失败：{ $err }
# Source: Please enter an instance name or select a version
instances-create-error-no-name = 请输入实例名称或选择一个版本
# Source: Failed to refresh instances: { $err }
instances-create-error-refresh-instances = 刷新实例列表失败：{ $err }
# Source: None
instances-create-none = 无
# Source: Refresh version list
instances-create-refresh-version = 刷新版本列表
# Source: Search versions...
instances-create-search-versions = 搜索版本...
# Source: Game Version
instances-create-settings-game-version = 游戏版本
# Source: Instance Name
instances-create-settings-instance-name = 实例名称
# Source: Instance Settings
instances-create-settings-title = 实例设置
# Source: Create
instances-list-button-create = 创建
# Source: Select an instance to launch Minecraft
instances-list-description = 选择一个用于启动 Minecraft 的实例
# Source: Instances
instances-list-title = 实例
# Source: Create an instance
instances-list-tooltip-create = 创建实例
# Source: System
language-system = 系统默认
# Source: Please select an account first
launch-error-account-not-select = 请先选择一个账户
# Source: Failed to verify asset index: { $e }
launch-error-asset-index = 验证资源索引失败：{ $e }
# Source: Failed to build launch command: { $e }
launch-error-build-command = 构建启动命令失败：{ $e }
# Source: Client ID not set
launch-error-client-id-empty = 未设置 Client ID
# Source: Instance "{ $name }" exited with code { $code }
launch-error-instance-exit-abnormal = 实例“{ $name }”异常退出，退出码：{ $code }
# Source: Please select an instance first
launch-error-instance-not-select = 请先选择一个实例
# Source: Java error: { $e }
launch-error-java = Java 错误：{ $e }
# Source: Failed to load version metadata: { $e }
launch-error-metadata = 加载版本元数据失败：{ $e }
# Source: Failed to prepare runtime: { $e }
launch-error-prepare-runtime = 准备运行环境失败：{ $e }
# Source: Failed to verify resources: { $e }
launch-error-resources = 验证资源文件失败：{ $e }
# Source: Runtime task execution error: { $e }
launch-error-runtime-execution = 运行时任务执行错误：{ $e }
# Source: Failed to save configuration: { $err }
launch-error-save-configuration = 保存配置失败：{ $err }
# Source: Failed to spawn game process: { $e }
launch-error-spawn-process = 启动游戏进程失败：{ $e }
# Source: UUIDs do not match
launch-error-uuid-mismatch = UUID 不匹配
# Source: Error waiting for instance "{ $name }": { $err }
launch-error-wait-instance = 等待实例“{ $name }”时出错：{ $err }
# Source: Authenticating account...
launch-stage-authenticating = 正在登入...
# Source: Verifying game files...
launch-stage-checking-integrity = 正在校验游戏文件...
# Source: Checking Java runtime...
launch-stage-checking-java = 正在检查 Java 运行时...
# Source: Launching Minecraft...
launch-stage-launching = 正在启动 Minecraft...
# Source: Loading version metadata...
launch-stage-loading-metadata = 正在加载版本元数据...
# Source: Preparing runtime...
launch-stage-prepare-runtime = 正在准备运行环境...
# Source: Waiting for game window...
launch-stage-waiting-window = 等待游戏窗口出现...
# Source: Remove instance
list-tooltip-remove = 移除实例
# Source: Instance settings
list-tooltip-settings = 实例设置
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
# Source: Cancel
ms-login-dialog-cancel = 取消
# Source: Copy Login Link
ms-login-dialog-copy-link = 复制登录链接
# Source: Microsoft Login
ms-login-dialog-heading = 微软登录
# Source: Authentication failed: { $e }
ms-login-error-auth = 认证失败：{ $e }
# Source: Microsoft Client ID is not configured
ms-login-error-client-id-empty = 未配置微软 Client ID
# Source: Failed to open browser automatically: { $err }
ms-login-error-open-browser = 自动打开浏览器失败：{ $err }
# Source: Failed to save accounts: { $err }
ms-login-error-save-accounts = 保存账户失败：{ $err }
# Source: Failed to start login session: { $err }
ms-login-error-start-session = 启动登录会话失败：{ $err }
# Source: Starting local login session...
ms-login-state-starting = 启动登录会话中...
# Source: Please complete the sign-in in your web browser.
ms-login-state-waiting-browser = 请在浏览器中完成登录。
# Source: Microsoft account added successfully
ms-login-toast-add-account = 微软账户添加成功
# Source: Login link copied to clipboard
ms-login-toast-copy-link = 登录链接已复制到剪贴板
# Source: Accounts
navigation-page-accounts = 账户
# Source: Home
navigation-page-home = 首页
# Source: Instances
navigation-page-instances = 实例
# Source: Settings
navigation-page-settings = 设置
# Source: You must launch the game at least once with an official Microsoft account before adding an offline account.
offline-disabled-dialog-body = 在添加离线账户之前，必须至少使用正版微软账户启动一次游戏。
# Source: Cancel
offline-disabled-dialog-cancel = 取消
# Source: Get Minecraft
offline-disabled-dialog-get-minecraft = 获取 Minecraft
# Source: Offline Mode Unavailable
offline-disabled-dialog-heading = 离线模式不可用
# Source: The Microsoft session for "{ $username }" has expired. Please sign in again.
reauth-dialog-body = “{ $username }”的微软会话已过期，请重新登录。
# Source: Cancel
reauth-dialog-cancel = 取消
# Source: Session Expired
reauth-dialog-heading = 会话已过期
# Source: Sign In
reauth-dialog-sign-in = 登录
# Source: How many files can be downloaded at the same time
settings-downloads-concurrent-subtitle = 允许同时下载的最大文件数
# Source: Max Concurrent Downloads
settings-downloads-concurrent-title = 最大并发下载数
# Source: Download Source
settings-downloads-source-title = 下载源
# Source: Downloads
settings-downloads-title = 下载
# Source: Failed to save configuration: { $err }
settings-error-save-configuration = 保存配置失败：{ $err }
# Source: Auto-detect (Recommended)
settings-java-auto-detect = 自动检测（推荐）
# Source: Max Memory
settings-java-max-memory-title = 最大内存
# Source: Rescan for installed Java runtimes
settings-java-rescan = 重新扫描已安装的 Java 运行时
# Source: Automatically match based on game version
settings-java-runtime-subtitle = 根据游戏版本自动匹配
# Source: Java Runtime
settings-java-runtime-title = Java 运行时
# Source: Java
settings-java-title = Java
# Source: Close Launcher On Launch
settings-launcher-close-title = 启动时关闭启动器
# Source: Set the display language.
settings-launcher-language-subtitle = 设置显示语言
# Source: Language
settings-launcher-language-title = 语言
# Source: Launcher
settings-launcher-title = 启动器
# Source: Successfully refreshed the Java list.
settings-toast-refresh-java = 成功刷新 Java 列表。
# Source: Username (e.g. Steve)
username-entry-placeholder-text = 用户名（例如 Steve）
# Source: Old
version-filter-old = 远古版本
# Source: Release
version-filter-release = 正式版
# Source: Snapshot
version-filter-snapshot = 快照版
