# RA3MCP

红警 3 控制与观测桥接：`RA3InputBridge.asi` 在游戏进程内转发鼠标输入，同时维护异步 AI data plane；外部 Rust `ra3ctl.exe` 通过命名管道控制移动、点击、拖动与滚轮，也可把最新 Skirmish AI 状态一次性导出为带时间戳的 JSON。鼠标工具同时提供原 CLI 和常驻 stdio MCP 服务；AI probe 当前只开放 CLI。

**状态：本机原生构建、原生测试与 Rust 全量测试通过；用户已确认游戏加载、CLI ping、点击和拖动有效。** 本机验证命令见 [MCP 接入规范](docs/MCP_TOOLHUB.md)。Toolhub 的代码、配置和注册由专门会话负责。

## 目标版本

- 32 位 `ra3_1.12.game`，IDA image base `0x400000`。
- 文件 SHA-256：`dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0`。
- 模块加载后先核对架构、映像范围、文件哈希；不匹配时记录日志并拒绝安装 hook。
- 被控制和校验的进程为 `Data/ra3_1.12.game`；游戏进程的 Loader、配置和 `scripts` 位于 `Data`；当前采用已验证的根目录与 Data 双份部署，单目录初始化尝试未成功。

地址、对象关系和调用约定见 [逆向记录](docs/REVERSE_ENGINEERING.md)。

## AI data plane 与 probe

ASI 在 `SkirmishAI::Update` 返回后，把已确认的对象字段复制到固定容量的进程内 plane。每个 AI 通过 `SkirmishAI -> Player -> PlayerList[slot]` 双向关系验证，并按 `Player+0x20` 的 0..19 槽位号索引；裸指针不再作为 agent 身份。发布只尝试获取锁；若 CLI 恰好正在复制快照，本帧采样会被跳过，游戏线程不会等待。独立工作线程通过 `\\.\pipe\RA3MCP.AI.<PID>` 导出二进制快照，JSON 序列化和落盘均由外部 CLI 完成。

进入包含 Skirmish AI 的对局后执行：

```powershell
$gamePid = (Get-CimInstance Win32_Process -Filter "Name='ra3_1.12.game'").ProcessId
.\target\release\ra3ctl.exe --pid $gamePid probe
```

默认生成 `dumps/ra3-ai-probe-<PID>-<UTC>-g<generation>.json`。可用 `probe --output <file>` 指定文件，已有文件不会被覆盖。v3 JSON 固定分为 `rough_snapshot` 和 `raw_data`：前者把 Personality、战略/预算/建造状态、战略指令、预算账户和战术类翻译成稳定名称；后者保留地址、AssetId、float bits、vector 边界、目标记录和战术对象原始字。完整字段和二进制协议见 [AI Probe](docs/AI_PROBE.md)。

## 本机准备与构建

依赖下载链接、版本与用途见 [下载清单](docs/DOWNLOADS.md)。需要 MSVC x86 C++ 工具链、Windows SDK、CMake，以及 Windows MSVC Rust 工具链。Rust 控制器可编译为 x64；ASI 必须为 x86。

将收到的两个原始 ZIP 放到仓库 `downloads/`，先在本机执行离线准备脚本：

```powershell
.\scripts\Prepare-Dependencies.ps1
```

脚本核对固定 SHA-256，把 MinHook 源码放到 `third_party/minhook`，并从 Loader 包的 `dinput8.dll` 准备 `downloads/ual-win32/d3d9.dll`。它不编译、不下载，也不改游戏目录；遇到内容不同的现有文件会拒绝覆盖。ZIP 在其他位置时可传 `-MinHookZip` 和 `-LoaderZip`。

也可手工将 MinHook v1.3.4 源码解压、重命名为以下目录；不要求下载 MinHook 预编译库：

```text
RA3MCP/
  third_party/minhook/include/MinHook.h
  third_party/minhook/src/hook.c
  third_party/minhook/LICENSE.txt
```

在本机仓库根目录用 PowerShell 执行：

```powershell
.\scripts\Build.ps1
.\scripts\Test.ps1
.\scripts\Stage.ps1
```

脚本默认使用 Visual Studio 2022 CMake generator，可通过 `Build.ps1 -Generator 'Visual Studio 18 2026'` 适配已安装且受当前 CMake 支持的版本。MinHook 也可由 `-MinHookDir 'D:\deps\minhook-1.3.4'` 指定；打包时传同一路径。

Rust 无第三方 crate，脚本使用 `--locked --offline`。首次安装工具链本身仍需要 Rustup 下载。构建脚本不下载依赖，也不自动修改游戏目录。

## ASI 加载（WinMM，已验证双目录实验）

1. 退出游戏和启动器，启动参数使用 `-win -xres 1920 -yres 1080 -runver 1.12`，实际目标为 `Data/ra3_1.12.game`。
2. 2026-10-17 的成功实验在游戏根目录和 Data 各保留了一套 WinMM、global.ini 和 scripts。游戏 PID 39836 从 `Data/scripts/RA3InputBridge.asi` 初始化，通过固定 SHA-256 校验、安装 Hook 并创建管道。这一结果尚未证明根目录副本是必需条件。
3. 同一目录只部署一种 UAL 代理；本轮使用 WinMM。准备脚本仍生成 `downloads/ual-win32/d3d9.dll`，可将同一个已校验的通用 UAL Win32 loader 复制并改名为 `winmm.dll`，也可使用先前已验证的同一 WinMM 文件。
4. 在需要加载 UAL 的目录放置 `winmm.dll`、`global.ini` 和 `scripts/RA3InputBridge.asi`。`scripts` 需手动创建；配置来自 `config/global.ini.example`。已有的其他插件或配置不应被整目录覆盖。
5. 查看对应 ASI 目录下的 `RA3InputBridge.log`。游戏 PID 应出现宿主 `Data/RA3_1.12.game`、匹配的 SHA-256，以及 `Ready: \\.\pipe\RA3MCP.<PID>`。日志为追加写入，复制旧 scripts 时也会带入旧启动器记录，应按本轮 PID 判断。

以下路径相对于包含 RA3.exe 的游戏根目录：

| 文件 | 已验证的游戏进程位置 |
| --- | --- |
| UAL loader | `Data/winmm.dll` |
| UAL 配置 | `Data/global.ini` |
| 桥接插件 | `Data/scripts/RA3InputBridge.asi` |
| 运行日志 | `Data/scripts/RA3InputBridge.log` |

移出根目录副本后的跟踪已看到游戏 PID 40564 加载 `Data/winmm.dll` 和系统 WinMM，但没有映射 ASI。因此不能把“代理已加载”当成“插件已初始化”。UAL 默认通过 IAT API 拦截延迟加载插件，拦截未成功时可能只转发系统 DLL；具体触发原因还待区分。两次实验也可能存在调试器和普通启动的差异。

单目录的延迟初始化和 `DontLoadFromDllMain=0` / `ForceEntryPointHook=0` 直接初始化尝试均未成功；现阶段固定已验证的双目录布局，并恢复 `DontLoadFromDllMain=1`。排查记录保留在 [启动链诊断](docs/LAUNCH_TRACE.md)，本轮继续 MCP 接入，不再调整加载策略。

ASI 通过 `InitializeASI` 启动工作线程，`DllMain` 不安装 hook。模块初始化后固定驻留至进程退出；**首版没有热卸载**，替换插件前退出游戏。卸载时移走新增加的 ASI/loader，按备份恢复已有文件。

## 加载进程与版本诊断

UAL 在游戏根目录使用 D3D9 或 WinMM 代理均已有用户加载测试。当前准备脚本默认生成 `d3d9.dll`；如使用已验证的 WinMM 路径，可将同一个已校验的 loader 改名为 `winmm.dll`。同一目录只部署一种 UAL 代理；游戏进程的配置与 ASI 在 Data，根目录副本目前属于成功对照实验的一部分。

启动器 `RA3.exe` 和 `Data/ra3_1.12.game` 是独立进程。ASI 被启动器加载后会记录 `Skipped launcher RA3.exe` 并跳过全部游戏 Hook；这不会替子进程加载 DLL。游戏进程需要自行加载 Data 的 UAL 和 ASI。

`Data/scripts/RA3InputBridge.log` 每行带 `[pid=...]`，每次初始化记录宿主可执行文件与 ASI 的完整路径。游戏校验还会记录 PE 架构、映像大小、实际与预期 SHA-256；文件读取和 BCrypt 失败分别给出 Win32 error 或 NTSTATUS。

- 只有 `Host executable: ...RA3.exe` 和 `Skipped launcher`：当前日志只证明启动器初始化了桥接，还需确认游戏子进程是否加载代理。
- 有 `Host executable: ...Data/ra3_1.12.game` 且出现哈希不匹配：ASI 已进入游戏进程，继续核对实际二进制版本；保留原版本约束。
- 出现文件读取或 BCrypt 错误：按日志中的具体错误排查，不能归结为版本不符。
- 游戏 PID 出现 `Ready: \\.\pipe\RA3MCP.<PID>`：桥接管道已创建，再按下面步骤测试输入。


## 启动链跟踪

新增独立外部 `ra3trace.exe`，用 Windows 调试事件记录启动器和继承调试链的子进程、DLL 实际加载路径、调试输出、异常和退出。现有 Toolhub `cargo-build` 会同时构建它；`cargo-test` 包含子进程冒烟测试，不启动游戏。

手动跟踪命令、日志位置和范围限制见 [启动链跟踪](docs/LAUNCH_TRACE.md)。它不会注入桥接；双目录部署已通过游戏加载与 CLI 输入验收，后续诊断仍以对应游戏 PID 的日志为准。

## 常驻 MCP 服务

`ra3ctl` 0.2.0 同时提供 CLI 与 stdio MCP 服务。由 Toolhub 托管一个长期运行的子进程：

```powershell
.\target\release\ra3ctl.exe mcp --transport stdio
```

服务支持 MCP 2026-07-28，以及 2025-11-25 / 2025-06-18 握手客户端。首次列工具不连接游戏；每次鼠标工具显式传 PID。缓存独占管道可用 `ra3_disconnect` 交还 CLI；点击/拖动串行执行，无额外切前台等待。

完整参数、生命周期、错误、取消及配置见 [Toolhub MCP 接入规范](docs/MCP_TOOLHUB.md)，[客户端配置示例](config/mcp-client.example.json) 和 [机器可读工具清单](controller/mcp-tools.json)。

2026-10-17 用户已在保留根目录与 Data 副本的布局下确认原 CLI 的 ping、点击、拖动通过；单目录初始化尝试未成功，本轮维持该部署基线，后续验收 MCP 调用。

## 点击和拖动

用日志里的游戏 PID 替换下面的 `12345`。坐标是游戏窗口客户区像素；不包含标题栏和窗口边框。

```powershell
.\dist\Release\ra3ctl.exe --pid 12345 ping
.\dist\Release\ra3ctl.exe --pid 12345 move 800 450
.\dist\Release\ra3ctl.exe --pid 12345 click left 800 450
.\dist\Release\ra3ctl.exe --pid 12345 click right 850 500 100
.\dist\Release\ra3ctl.exe --pid 12345 drag left 300 250 700 550 600 30
.\dist\Release\ra3ctl.exe --pid 12345 wheel 800 450 -1
.\dist\Release\ra3ctl.exe --pid 12345 release
```

拖动末尾的 `600 30` 表示期望 600ms、30 个移动采样。每次指令等待输入循环回执；低帧率会延长实际耗时。成功打印 `pumped sequence=... pump=... held=0`，只表示输入循环已处理指令，不能据此认定游戏选中了单位或接受了命令。

当前修改的是游戏内部鼠标输入，**不保证移动 Windows 系统光标**。第一轮验证请保持游戏在前台、不要同时操作实体鼠标；原生输入仍在运行，可能覆盖合成输入。后台控制、独占输入、按键修饰符及游戏逻辑命令不在本版承诺内。

同一游戏只接受一个管道连接。连接断开或指令超时后，会请求在下一次游戏输入循环释放本插件记录的按键；游戏输入线程停住时，释放也必须等待它恢复。超时的点击或拖动可能已执行一部分，不要自动重试。

## 验证与后续接入

本机编译命令和验收步骤见 [MCP 接入规范](docs/MCP_TOOLHUB.md)，历史构建操作参考见 [本地验证计划](docs/TOOLHUB.md)。[桥接协议文档](docs/PROTOCOL.md) 描述控制器与 ASI 之间的 24 字节命名管道协议。

项目采用 Apache-2.0；静态链接的 MinHook 保留其 BSD 许可证，`Stage.ps1` 会一并复制。ASI Loader 单独下载并遵循其许可证。仓库不包含游戏二进制、游戏资源或第三方预编译 DLL。

